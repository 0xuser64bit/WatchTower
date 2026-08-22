//! The monitoring loop.
//!
//! One tick reads every enabled rule, resolves the current value of each distinct
//! target, evaluates, and dispatches. Two properties matter most:
//!
//! * **Failures are isolated per rule.** One unreachable target or delivery failure
//!   does not skip every remaining rule for that interval.
//! * **Targets are read once per tick, not once per rule.** Values are resolved for
//!   distinct targets up front, and wallet balances are fetched in a single batched
//!   RPC call rather than one request per wallet.

use crate::app_state::AppState;
use crate::db::repos::alert_events::AlertEventRepo;
use crate::db::repos::rules::{EvaluationOutcome, RuleRepo};
use crate::engine::status::TickReport;
use crate::rules::eval::{evaluate, Decision, StateChange};
use crate::rules::types::{Rule, RuleState, TargetKind};
use chrono::Utc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;
/// How often expired alert history is pruned.
const PRUNE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

pub async fn run(state: AppState) {
    let interval = state.settings.poll_interval;
    state.status.mark_started();

    info!(
        interval_secs = interval.as_secs(),
        "monitoring engine started"
    );

    let mut ticker = tokio::time::interval(interval);
    // Falling behind must not queue up a burst of catch-up ticks against
    // rate-limited providers.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut last_prune = Instant::now() - PRUNE_EVERY;

    loop {
        tokio::select! {
            biased;

            _ = state.shutdown.cancelled() => {
                info!("monitoring engine received shutdown signal");
                break;
            }

            _ = ticker.tick() => {
                let started = Instant::now();

                match tick(&state).await {
                    Ok(report) => {
                        let elapsed = started.elapsed();
                        state.status.record_tick(report, elapsed);
                        debug!(
                            rules = report.rules_evaluated,
                            alerts = report.alerts_sent,
                            unavailable = report.targets_unavailable,
                            duration_ms = elapsed.as_millis(),
                            "tick complete"
                        );

                        if elapsed > interval {
                            warn!(
                                duration_ms = elapsed.as_millis(),
                                interval_ms = interval.as_millis(),
                                "tick took longer than the poll interval; consider raising POLL_INTERVAL_SECONDS"
                            );
                        }
                    }
                    Err(err) => {
                        // Only whole-tick failures land here (e.g. the rule list
                        // could not be read); individual rules are handled inline.
                        warn!(%err, "monitoring tick failed");
                        state.status.record_tick_failure(&err);
                    }
                }

                if last_prune.elapsed() >= PRUNE_EVERY {
                    last_prune = Instant::now();
                    prune_history(&state).await;
                }
            }
        }
    }
}

async fn prune_history(state: &AppState) {
    let days = state.settings.alert_history_retention_days;

    match AlertEventRepo::new(&state.db)
        .prune_older_than_days(days)
        .await
    {
        Ok(0) => {}
        Ok(removed) => info!(removed, retention_days = days, "pruned alert history"),
        Err(err) => warn!(%err, "failed to prune alert history"),
    }
}

/// Runs a single monitoring cycle.
///
/// Public so the whole data plane — value resolution, evaluation, persistence, and
/// dispatch — can be exercised deterministically by tests instead of only through
/// the timer loop.
pub async fn tick(state: &AppState) -> crate::error::Result<TickReport> {
    let rules = RuleRepo::new(&state.db).list_enabled().await?;

    if rules.is_empty() {
        return Ok(TickReport::default());
    }

    let values = resolve_target_values(state, &rules).await;

    let mut report = TickReport {
        targets_unavailable: values.iter().filter(|(_, v)| v.is_none()).count(),
        ..TickReport::default()
    };

    for rule in &rules {
        let key = target_key(rule);

        let Some(Some(observed)) = values.get(&key) else {
            // The target's value was unavailable this tick. The rule keeps its
            // existing state, so a firing rule is not wrongly re-armed by a
            // provider outage.
            continue;
        };

        report.rules_evaluated += 1;

        if let Err(err) = process_rule(state, rule, *observed, &mut report).await {
            warn!(rule_id = rule.id, %err, "failed to process rule");
        }
    }

    Ok(report)
}

/// Evaluates one rule, dispatches if needed, and persists the outcome.
async fn process_rule(
    state: &AppState,
    rule: &Rule,
    observed: f64,
    report: &mut TickReport,
) -> crate::error::Result<()> {
    let now = Utc::now();
    let decision = evaluate(rule, observed, now);

    if let Decision::Skip { reason } = decision {
        warn!(rule_id = rule.id, reason, "skipping rule evaluation");
        return Ok(());
    }

    let mut triggered_at = None;

    if decision.should_notify() {
        match state.dispatcher.dispatch(rule, &decision, now).await {
            Ok(Some(delivery)) => {
                report.alerts_sent += 1;
                triggered_at = Some(now);
                debug!(
                    rule_id = rule.id,
                    event_id = delivery.event_id,
                    "alert recorded"
                );
            }
            Ok(None) => {}
            Err(err) => {
                // Leave the rule un-triggered so the next tick retries. Delivery is
                // therefore at-least-once rather than at-most-once, which is the
                // right trade-off for an alerting system.
                warn!(rule_id = rule.id, %err, "failed to dispatch alert; will retry next tick");
                return Err(err);
            }
        }
    }

    let state_change = match decision.state_change() {
        StateChange::ToFiring if rule.state != RuleState::Firing => Some(RuleState::Firing),
        StateChange::ToOk if rule.state != RuleState::Ok => Some(RuleState::Ok),
        _ => None,
    };

    let reference_value = match decision {
        // First observation: establish the baseline.
        Decision::BaselineSet { reference } => Some(reference),
        // A percentage rule that just fired re-baselines to the current value, so it
        // measures the *next* move rather than staying anchored to a stale reference
        // and re-firing forever.
        Decision::Notify { observed, .. } if rule.operator.is_percentage() => Some(observed),
        _ => None,
    };

    RuleRepo::new(&state.db)
        .record_evaluation(
            rule.id,
            EvaluationOutcome {
                observed,
                evaluated_at: now,
                state: state_change,
                reference_value,
                triggered_at,
            },
        )
        .await
}

/// Identifies a distinct thing to read, so two rules on the same target share a read.
fn target_key(rule: &Rule) -> (TargetKind, i64) {
    (rule.target.kind, rule.target.id)
}

/// Reads the current value of every distinct target referenced by `rules`.
async fn resolve_target_values(
    state: &AppState,
    rules: &[Rule],
) -> HashMap<(TargetKind, i64), Option<f64>> {
    let mut values = HashMap::new();

    let mut mints: Vec<(i64, String)> = Vec::new();
    let mut wallets: Vec<(i64, String)> = Vec::new();

    for rule in rules {
        let key = target_key(rule);
        if values.contains_key(&key) {
            continue;
        }
        values.insert(key, None);

        match rule.target.kind {
            TargetKind::Token => mints.push((rule.target.id, rule.target.reference.clone())),
            TargetKind::Wallet => wallets.push((rule.target.id, rule.target.reference.clone())),
        }
    }

    // Prices: one request per mint, since CoinGecko's public tier rejects batches.
    let mut price_ok = 0usize;
    for (token_id, mint) in &mints {
        match state.price_provider.get_token_price_usd(mint).await {
            Ok(price) => {
                price_ok += 1;
                values.insert((TargetKind::Token, *token_id), Some(price));
            }
            Err(err) => warn!(mint = %mint, %err, "failed to read token price"),
        }
    }

    if !mints.is_empty() {
        state.status.set_price_provider_healthy(price_ok > 0);
    }

    // Balances: a single batched call covers up to 100 wallets.
    if !wallets.is_empty() {
        let addresses: Vec<String> = wallets.iter().map(|(_, a)| a.clone()).collect();

        match state
            .chain_provider
            .get_native_balances_lamports(&addresses)
            .await
        {
            Ok(balances) => {
                state.status.set_chain_provider_healthy(true);
                for ((wallet_id, _), lamports) in wallets.iter().zip(balances) {
                    values.insert(
                        (TargetKind::Wallet, *wallet_id),
                        Some(lamports as f64 / LAMPORTS_PER_SOL),
                    );
                }
            }
            Err(err) => {
                state.status.set_chain_provider_healthy(false);
                warn!(wallets = wallets.len(), %err, "failed to read wallet balances");
            }
        }
    }

    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::types::RuleTarget;

    fn rule(id: i64, kind: TargetKind, target_id: i64) -> Rule {
        Rule {
            id,
            target: RuleTarget {
                kind,
                id: target_id,
                reference: format!("target-{target_id}"),
                label: None,
            },
            operator: crate::rules::types::Operator::Gt,
            threshold: 1.0,
            cooldown_seconds: 0,
            reference_value: None,
            state: RuleState::Ok,
            enabled: true,
            last_value: None,
            last_evaluated_at: None,
            last_triggered_at: None,
        }
    }

    #[test]
    fn rules_on_the_same_target_share_one_read() {
        let rules = [
            rule(1, TargetKind::Token, 10),
            rule(2, TargetKind::Token, 10),
            rule(3, TargetKind::Wallet, 10),
        ];

        let keys: std::collections::HashSet<_> = rules.iter().map(target_key).collect();

        // Same token twice collapses to one read; the wallet with the same numeric id
        // is a different target and must not collide.
        assert_eq!(keys.len(), 2);
    }
}
