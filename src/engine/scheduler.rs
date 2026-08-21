use crate::app_state::AppState;
use crate::db::repos::rules::RuleRepo;
use crate::rules::evaluate;
use crate::rules::types::{Operator, RuleKind, RuleOutcome, Sample};
use std::collections::HashMap;
use teloxide::types::ChatId;
use tracing::{debug, info, warn};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

pub async fn run(state: AppState, admin_chat_id: ChatId) {
    let interval = state.settings.poll_interval;

    info!(
        interval_secs = interval.as_secs(),
        "monitoring engine started"
    );

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = state.shutdown.cancelled() => {
                info!("monitoring engine received shutdown signal");
                break;
            }
            _ = ticker.tick() => {
                if let Err(err) = run_tick(&state, admin_chat_id).await {
                    warn!(%err, "monitoring tick failed");
                }
            }
        }
    }
}

async fn run_tick(state: &AppState, admin_chat_id: ChatId) -> crate::error::Result<()> {
    let rules = RuleRepo::new(&state.db).list_enabled().await?;
    debug!(rule_count = rules.len(), "evaluating rules");

    let mut token_cache: HashMap<String, f64> = HashMap::new();

    for rule in &rules {
        let sample = match fetch_sample(state, rule, &mut token_cache).await {
            Ok(Some(sample)) => sample,
            Ok(None) => continue,
            Err(err) => {
                warn!(rule_id = rule.id, %err, "failed to fetch sample");
                continue;
            }
        };

        if is_percentage_rule(rule) && rule.reference_value.is_none() {
            RuleRepo::new(&state.db)
                .initialize_reference_if_missing(rule.id, sample.value)
                .await?;
            continue;
        }

        let outcome = evaluate(rule, &sample);

        if let RuleOutcome::Trigger { current, threshold } = outcome {
            state
                .dispatcher
                .dispatch(rule, current, threshold, admin_chat_id)
                .await?;
        }
    }

    Ok(())
}

async fn fetch_sample(
    state: &AppState,
    rule: &crate::rules::types::Rule,
    token_cache: &mut HashMap<String, f64>,
) -> crate::error::Result<Option<Sample>> {
    let value = match rule.kind() {
        RuleKind::Price => {
            if let Some(cached) = token_cache.get(&rule.target_ref) {
                *cached
            } else {
                let value = state
                    .price_provider
                    .get_token_price_usd(&rule.target_ref)
                    .await?;
                token_cache.insert(rule.target_ref.clone(), value);
                value
            }
        }
        RuleKind::Balance => {
            let lamports = state
                .chain_provider
                .get_native_balance_lamports(&rule.target_ref)
                .await?;
            lamports as f64 / LAMPORTS_PER_SOL
        }
    };

    Ok(Some(Sample {
        value,
        reference: rule.reference_value,
    }))
}

fn is_percentage_rule(rule: &crate::rules::types::Rule) -> bool {
    matches!(
        rule.operator(),
        Operator::PctChangeUp | Operator::PctChangeDown
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppState;
    use crate::config::Settings;
    use crate::db::Db;
    use crate::providers::{ChainProvider, PriceProvider, ProviderResult};
    use async_trait::async_trait;
    use std::sync::Arc;
    use teloxide::Bot;
    use tokio_util::sync::CancellationToken;

    struct MockPrice {
        token: f64,
    }

    #[async_trait]
    impl PriceProvider for MockPrice {
        async fn get_native_price_usd(&self) -> ProviderResult<f64> {
            Ok(self.token)
        }

        async fn get_token_price_usd(&self, _mint: &str) -> ProviderResult<f64> {
            Ok(self.token)
        }
    }

    struct MockChain {
        lamports: u64,
    }

    #[async_trait]
    impl ChainProvider for MockChain {
        async fn get_native_balance_lamports(&self, _address: &str) -> ProviderResult<u64> {
            Ok(self.lamports)
        }
    }

    fn settings() -> Arc<Settings> {
        Arc::new(
            Settings::from_env_map(&std::collections::HashMap::from([
                (
                    "TELEGRAM_BOT_TOKEN".to_string(),
                    "1234567890:test-token".to_string(),
                ),
                ("ADMIN_TELEGRAM_IDS".to_string(), "1".to_string()),
                ("DATABASE_URL".to_string(), "sqlite::memory:".to_string()),
            ]))
            .expect("valid test settings"),
        )
    }

    fn state(db: Arc<Db>, price: f64, lamports: u64) -> AppState {
        let bot = Bot::new("test");
        AppState::new(
            db.clone(),
            bot,
            settings(),
            Arc::new(MockPrice { token: price }),
            Arc::new(MockChain { lamports }),
            CancellationToken::new(),
        )
    }

    fn rule(operator: &str, kind: &str, target_type: &str) -> crate::rules::types::Rule {
        use chrono::Utc;
        crate::rules::types::Rule {
            id: 1,
            kind: kind.into(),
            target_type: target_type.into(),
            target_ref: "addr".into(),
            metric: if kind == "price" { "price" } else { "balance" }.into(),
            operator: operator.into(),
            threshold: 10.0,
            time_window_seconds: None,
            cooldown_seconds: 300,
            max_triggers: None,
            reference_value: None,
            enabled: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[tokio::test]
    async fn fetch_sample_returns_sol_for_balance() {
        let db = Arc::new(Db::connect_in_memory().await.unwrap());
        db.migrate().await.unwrap();
        let state = state(db, 2.0, 2_500_000_000);
        let rule = rule(">", "balance", "wallet");

        let sample = fetch_sample(&state, &rule, &mut HashMap::new())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(sample.value, 2.5);
    }

    #[tokio::test]
    async fn fetch_sample_caches_token_prices() {
        let db = Arc::new(Db::connect_in_memory().await.unwrap());
        db.migrate().await.unwrap();
        let state = state(db, 4.0, 0);
        let rule = rule(">", "price", "token");
        let mut cache = HashMap::new();

        let first = fetch_sample(&state, &rule, &mut cache)
            .await
            .unwrap()
            .unwrap();
        let second = fetch_sample(&state, &rule, &mut cache)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.value, 4.0);
        assert_eq!(second.value, 4.0);
    }
}
