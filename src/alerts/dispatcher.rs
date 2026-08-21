use crate::alerts::format::format_alert;
use crate::db::repos::alert_events::AlertEventRepo;
use crate::db::repos::rules::RuleRepo;
use crate::db::repos::users::UserRepo;
use crate::db::Db;
use crate::error::Result;
use crate::rules::types::Rule;
use chrono::{Duration as ChronoDuration, Utc};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatId;
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
        fallback_chat_id: ChatId,
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

        self.send_to_admins(message, fallback_chat_id).await;

        Ok(())
    }

    async fn send_to_admins(&self, message: String, fallback_chat_id: ChatId) {
        let admins = match UserRepo::new(&self.db).list_active_admins().await {
            Ok(admins) => admins,
            Err(err) => {
                warn!(%err, "failed to load admin recipients");
                return;
            }
        };

        if admins.is_empty() {
            if let Err(err) = self.bot.send_message(fallback_chat_id, message).await {
                warn!(%err, "failed to send fallback telegram alert");
            }
            return;
        }

        for admin in admins {
            if let Err(err) = self
                .bot
                .send_message(ChatId(admin.telegram_id), &message)
                .await
            {
                warn!(telegram_id = admin.telegram_id, %err, "failed to send telegram alert");
            }
        }
    }
}

fn build_dedup_key(rule: &Rule) -> String {
    let scope = if let Some(window_secs) = rule.time_window_seconds {
        format!("window:{}", Utc::now().timestamp() / window_secs)
    } else {
        format!("tick:{}", Utc::now().timestamp())
    };

    let raw = format!("{}|{}|{}", rule.id, rule.metric, scope);
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

    fn rule(window_seconds: Option<i64>) -> Rule {
        Rule {
            id: 42,
            kind: RuleKind::Price.as_str().into(),
            target_type: TargetType::Token.as_str().into(),
            target_ref: "mint".into(),
            metric: "price".into(),
            operator: ">".into(),
            threshold: 100.0,
            time_window_seconds: window_seconds,
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
    fn dedup_key_is_stable_within_window() {
        let key1 = build_dedup_key(&rule(Some(60)));
        let key2 = build_dedup_key(&rule(Some(60)));
        assert_eq!(key1, key2);
        assert_eq!(key1.len(), 64);
    }

    #[test]
    fn dedup_key_is_second_based_without_window() {
        let key1 = build_dedup_key(&rule(None));
        let key2 = build_dedup_key(&rule(None));
        assert_eq!(key1, key2);
    }
}
