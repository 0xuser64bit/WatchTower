use crate::app_state::AppState;
use crate::db::repos::rules::RuleRepo;
use crate::providers::{ChainProvider, PriceProvider};
use crate::rules::types::{Operator, RuleKind, RuleOutcome, Sample, TargetType};
use crate::rules::evaluate;
use std::collections::HashMap;
use std::time::Duration;
use teloxide::types::ChatId;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

pub async fn run(state: AppState, admin_chat_id: ChatId) {
    let interval = Duration::from_secs(state.settings.poll_interval_seconds);

    info!(interval_secs = interval.as_secs(), "monitoring engine started");

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

    let mut native_cache: HashMap<String, f64> = HashMap::new();
    let mut token_cache: HashMap<String, f64> = HashMap::new();

    for rule in &rules {
        let sample = match fetch_sample(state, rule, &mut native_cache, &mut token_cache).await {
            Ok(Some(sample)) => sample,
            Ok(None) => continue,
            Err(err) => {
                warn!(rule_id = rule.id, %err, "failed to fetch sample");
                continue;
            }
        };

        let outcome = evaluate(rule, &sample);

        if let RuleOutcome::Trigger { current, threshold } = outcome {
            state
                .dispatcher
                .dispatch(rule, current, threshold, admin_chat_id)
                .await?;

            if let Some(reference) = should_update_reference(rule) {
                if reference {
                    RuleRepo::new(&state.db)
                        .set_reference_value(rule.id, current)
                        .await?;
                }
            }
        } else if should_update_reference(rule).unwrap_or(false) {
            RuleRepo::new(&state.db)
                .set_reference_value(rule.id, sample.value)
                .await?;
        }
    }

    Ok(())
}

async fn fetch_sample(
    state: &AppState,
    rule: &crate::rules::types::Rule,
    native_cache: &mut HashMap<String, f64>,
    token_cache: &mut HashMap<String, f64>,
) -> crate::error::Result<Option<Sample>> {
    match rule.kind() {
        RuleKind::Price => {
            let price = if token_cache.contains_key(&rule.target_ref) {
                *token_cache.get(&rule.target_ref).unwrap()
            } else {
                let value = state
                    .price_provider
                    .get_token_price_usd(&rule.target_ref)
                    .await?;
                token_cache.insert(rule.target_ref.clone(), value);
                value
            };

            Ok(Some(Sample {
                value: price,
                reference: rule.reference_value,
            }))
        }
        RuleKind::Balance => {
            let balances = state
                .chain_provider
                .get_token_balances(&rule.target_ref)
                .await?;

            let value = balances
                .iter()
                .find(|balance| balance.mint == rule.target_ref)
                .map(|balance| balance.amount as f64 / 10_f64.powi(balance.decimals as i32))
                .unwrap_or(0.0);

            Ok(Some(Sample {
                value,
                reference: rule.reference_value,
            }))
        }
        RuleKind::Activity => {
            let signatures = state
                .chain_provider
                .get_recent_signatures(&rule.target_ref, 5)
                .await?;

            let activity = signatures.len() as f64;
            Ok(Some(Sample {
                value: activity,
                reference: None,
            }))
        }
    }
}

fn should_update_reference(rule: &crate::rules::types::Rule) -> Option<bool> {
    matches!(
        rule.operator(),
        Operator::PctChangeUp | Operator::PctChangeDown
    )
    .then_some(true)
}
