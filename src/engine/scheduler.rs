use crate::app_state::AppState;
use crate::db::repos::rules::RuleRepo;
use crate::rules::evaluate;
use crate::rules::types::{Operator, RuleKind, RuleOutcome, Sample};
use std::collections::HashMap;
use std::time::Duration;
use teloxide::types::ChatId;
use tracing::{debug, info, warn};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

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
