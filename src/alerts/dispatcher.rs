use crate::alerts::format::format_alert;
use crate::db::repos::alert_events::AlertEventRepo;
use crate::db::repos::rules::RuleRepo;
use crate::db::Db;
use crate::error::Result;
use crate::rules::types::Rule;
use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use teloxide::prelude::*;
use tracing::{debug, warn};

pub struct AlertDispatcher {
    bot: Bot,
    db: Arc<Db>,
}

impl AlertDispatcher {
    pub fn new(bot: Bot, db: Arc<Db>) -> Self {
        Self { bot, db }
    }

    pub async fn dispatch(
        &self,
        rule: &Rule,
        current: f64,
        threshold: f64,
        chat_id: ChatId,
    ) -> Result<()> {
        let event_repo = AlertEventRepo::new(&self.db);
        let rule_repo = RuleRepo::new(&self.db);

        if let Some(last_trigger) = rule_repo.last_trigger_at(rule.id).await? {
            let elapsed = Utc::now() - last_trigger;
            if elapsed < ChronoDuration::seconds(rule.cooldown_seconds) {
                debug!(rule_id = rule.id, "skipping alert within cooldown");
                return Ok(());
            }
        }

        if let (Some(max), Some(window_secs)) = (rule.max_triggers, rule.time_window_seconds) {
            let since = Utc::now() - ChronoDuration::seconds(window_secs);
            let count = rule_repo.count_triggers_since(rule.id, since).await?;
            if count >= max {
                debug!(rule_id = rule.id, count, "max triggers reached");
                return Ok(());
            }
        }

        let dedup_key = build_dedup_key(rule);
        if event_repo.find_by_dedup_key(&dedup_key).await?.is_some() {
            debug!(rule_id = rule.id, "duplicate alert suppressed");
            return Ok(());
        }

        let message = format_alert(rule, current, threshold);

        event_repo
            .insert(rule.id, current, threshold, &message, &dedup_key)
            .await?;

        if let Err(err) = self.bot.send_message(chat_id, message).await {
            warn!(rule_id = rule.id, %err, "failed to send telegram alert");
        }

        Ok(())
    }
}

fn build_dedup_key(rule: &Rule) -> String {
    let bucket = if let Some(window_secs) = rule.time_window_seconds {
        Utc::now().timestamp() / window_secs
    } else {
        0
    };

    let raw = format!("{}|{}|{}", rule.id, rule.metric, bucket);
    let hash = Sha256::digest(raw.as_bytes());
    hex_string(&hash)
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::types::{RuleKind, TargetType};
    use chrono::Utc;

    fn rule() -> Rule {
        Rule {
            id: 42,
            kind: RuleKind::Price.as_str().into(),
            target_type: TargetType::Token.as_str().into(),
            target_ref: "mint".into(),
            metric: "price".into(),
            operator: ">".into(),
            threshold: 100.0,
            time_window_seconds: Some(60),
            cooldown_seconds: 300,
            max_triggers: None,
            reference_value: None,
            enabled: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn dedup_key_is_deterministic_within_window() {
        let key1 = build_dedup_key(&rule());
        let key2 = build_dedup_key(&rule());
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 64);
    }
}
