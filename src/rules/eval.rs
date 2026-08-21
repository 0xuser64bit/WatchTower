//! Rule evaluation.
//!
//! Pure functions over an observed value and the rule's stored state. Deciding
//! *whether* to notify lives here so it can be exhaustively tested without a
//! database, a clock, or a Telegram connection.
//!
//! Alerting is **edge-triggered**. A rule fires on the transition from `Ok` to
//! `Firing` and then stays quiet while the condition remains true. The previous
//! implementation compared a time-bucketed SHA-256 of the rule against a UNIQUE
//! column, which meant a rule whose condition stayed true re-notified on every
//! cooldown expiry indefinitely, while a rule that genuinely re-crossed its
//! threshold inside the same clock bucket was suppressed. Cooldown is retained as a
//! secondary rate limit for conditions that oscillate across the threshold.

use crate::rules::types::{Operator, Rule, RuleState};
use chrono::{DateTime, Duration, Utc};

/// What the scheduler should do with a rule after observing a value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decision {
    /// Condition is true and the alert should be delivered now.
    Notify {
        observed: f64,
        /// The value compared against: the absolute threshold, or the baseline for
        /// percentage operators.
        reference: Option<f64>,
    },
    /// Condition is true but this rule already notified and has not recovered.
    AlreadyFiring,
    /// Condition is true, but the rule re-armed within its cooldown window.
    Suppressed { retry_after_seconds: i64 },
    /// Condition is false. The rule is armed (again).
    Clear,
    /// First observation for a percentage rule: records the baseline only.
    BaselineSet { reference: f64 },
    /// The observation cannot be evaluated (non-finite reading, unusable baseline).
    Skip { reason: &'static str },
}

/// State transition to persist for a rule after a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateChange {
    None,
    ToFiring,
    ToOk,
}

impl Decision {
    pub fn state_change(&self) -> StateChange {
        match self {
            Decision::Notify { .. } => StateChange::ToFiring,
            // A rule inside its cooldown is still conditionally firing; marking it
            // so prevents a burst of alerts the moment the cooldown lapses.
            Decision::Suppressed { .. } => StateChange::ToFiring,
            Decision::Clear => StateChange::ToOk,
            Decision::AlreadyFiring | Decision::BaselineSet { .. } | Decision::Skip { .. } => {
                StateChange::None
            }
        }
    }

    pub fn should_notify(&self) -> bool {
        matches!(self, Decision::Notify { .. })
    }
}

/// Evaluates `rule` against `observed` at time `now`.
pub fn evaluate(rule: &Rule, observed: f64, now: DateTime<Utc>) -> Decision {
    // A NaN or infinite reading means the provider returned something unusable.
    // Comparing it would silently yield `false` and clear a genuinely firing rule.
    if !observed.is_finite() {
        return Decision::Skip {
            reason: "provider returned a non-finite value",
        };
    }

    // Matched exhaustively in one place so there is no unreachable arm to panic on,
    // and so adding an operator is a compile error rather than a silent fallthrough.
    let condition_met = match rule.operator {
        Operator::Gt => observed > rule.threshold,
        Operator::Lt => observed < rule.threshold,
        Operator::Gte => observed >= rule.threshold,
        Operator::Lte => observed <= rule.threshold,
        Operator::PctUp | Operator::PctDown => {
            let Some(baseline) = rule.reference_value else {
                return Decision::BaselineSet {
                    reference: observed,
                };
            };

            // A zero or non-finite baseline makes percentage change undefined.
            if baseline == 0.0 || !baseline.is_finite() {
                return Decision::Skip {
                    reason: "baseline is not usable for percentage change",
                };
            }

            let change_pct = (observed - baseline) / baseline.abs() * 100.0;

            if rule.operator == Operator::PctUp {
                change_pct >= rule.threshold
            } else {
                change_pct <= -rule.threshold
            }
        }
    };

    if !condition_met {
        return Decision::Clear;
    }

    // Edge trigger: already-firing rules stay quiet until they recover.
    if rule.state == RuleState::Firing {
        return Decision::AlreadyFiring;
    }

    if let Some(last) = rule.last_triggered_at {
        let elapsed = now.signed_duration_since(last);
        let cooldown = Duration::seconds(rule.cooldown_seconds);
        // Guard against a negative elapsed time, which happens if the host clock
        // steps backwards; treating it as "cooled down" would defeat rate limiting.
        if elapsed < cooldown {
            let retry_after_seconds = (cooldown - elapsed).num_seconds().max(0);
            return Decision::Suppressed {
                retry_after_seconds,
            };
        }
    }

    Decision::Notify {
        observed,
        reference: if rule.operator.is_percentage() {
            rule.reference_value
        } else {
            Some(rule.threshold)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::types::{RuleTarget, TargetKind};

    fn rule(operator: Operator, threshold: f64) -> Rule {
        Rule {
            id: 1,
            target: RuleTarget {
                kind: TargetKind::Token,
                id: 1,
                reference: "MINT".into(),
                label: None,
            },
            operator,
            threshold,
            cooldown_seconds: 300,
            reference_value: None,
            state: RuleState::Ok,
            enabled: true,
            last_value: None,
            last_evaluated_at: None,
            last_triggered_at: None,
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn threshold_operators_respect_strictness_at_the_boundary() {
        let cases = [
            (Operator::Gt, 10.0, false),
            (Operator::Gte, 10.0, true),
            (Operator::Lt, 10.0, false),
            (Operator::Lte, 10.0, true),
        ];

        for (operator, observed, expect_notify) in cases {
            let decision = evaluate(&rule(operator, 10.0), observed, now());
            assert_eq!(
                decision.should_notify(),
                expect_notify,
                "{operator:?} at the exact threshold"
            );
        }
    }

    #[test]
    fn notifies_when_crossing_and_reports_the_comparison() {
        let decision = evaluate(&rule(Operator::Gt, 10.0), 11.0, now());
        assert_eq!(
            decision,
            Decision::Notify {
                observed: 11.0,
                reference: Some(10.0)
            }
        );
        assert_eq!(decision.state_change(), StateChange::ToFiring);
    }

    #[test]
    fn firing_rule_does_not_renotify_while_condition_holds() {
        let mut rule = rule(Operator::Gt, 10.0);
        rule.state = RuleState::Firing;

        let decision = evaluate(&rule, 50.0, now());
        assert_eq!(decision, Decision::AlreadyFiring);
        assert_eq!(decision.state_change(), StateChange::None);
    }

    #[test]
    fn firing_rule_rearms_once_the_condition_clears() {
        let mut rule = rule(Operator::Gt, 10.0);
        rule.state = RuleState::Firing;

        let decision = evaluate(&rule, 5.0, now());
        assert_eq!(decision, Decision::Clear);
        assert_eq!(decision.state_change(), StateChange::ToOk);
    }

    #[test]
    fn recovered_rule_notifies_again_after_cooldown() {
        let mut rule = rule(Operator::Gt, 10.0);
        rule.state = RuleState::Ok;
        rule.last_triggered_at = Some(now() - Duration::seconds(600));

        assert!(evaluate(&rule, 11.0, now()).should_notify());
    }

    #[test]
    fn recovered_rule_is_rate_limited_inside_cooldown() {
        let mut rule = rule(Operator::Gt, 10.0);
        rule.state = RuleState::Ok;
        rule.last_triggered_at = Some(now() - Duration::seconds(60));

        let decision = evaluate(&rule, 11.0, now());
        assert_eq!(
            decision,
            Decision::Suppressed {
                retry_after_seconds: 240
            }
        );
        // Must be marked firing, otherwise the alert fires the instant cooldown ends
        // even though this is the same continuous breach.
        assert_eq!(decision.state_change(), StateChange::ToFiring);
    }

    #[test]
    fn zero_cooldown_allows_immediate_reflapping() {
        let mut rule = rule(Operator::Gt, 10.0);
        rule.cooldown_seconds = 0;
        rule.last_triggered_at = Some(now());

        assert!(evaluate(&rule, 11.0, now()).should_notify());
    }

    #[test]
    fn first_percentage_observation_only_records_a_baseline() {
        let decision = evaluate(&rule(Operator::PctUp, 10.0), 100.0, now());
        assert_eq!(decision, Decision::BaselineSet { reference: 100.0 });
        assert_eq!(decision.state_change(), StateChange::None);
    }

    #[test]
    fn percentage_change_is_measured_against_the_baseline() {
        let mut up = rule(Operator::PctUp, 10.0);
        up.reference_value = Some(100.0);
        assert!(evaluate(&up, 110.0, now()).should_notify());
        assert!(!evaluate(&up, 109.0, now()).should_notify());

        let mut down = rule(Operator::PctDown, 10.0);
        down.reference_value = Some(100.0);
        assert!(evaluate(&down, 90.0, now()).should_notify());
        assert!(!evaluate(&down, 91.0, now()).should_notify());
    }

    #[test]
    fn percentage_notify_reports_the_baseline_not_the_threshold() {
        let mut up = rule(Operator::PctUp, 10.0);
        up.reference_value = Some(100.0);
        assert_eq!(
            evaluate(&up, 120.0, now()),
            Decision::Notify {
                observed: 120.0,
                reference: Some(100.0)
            }
        );
    }

    #[test]
    fn unusable_baseline_is_skipped_not_treated_as_clear() {
        let mut up = rule(Operator::PctUp, 10.0);
        up.reference_value = Some(0.0);
        assert!(matches!(evaluate(&up, 50.0, now()), Decision::Skip { .. }));
        // Crucially not `Clear`, which would wrongly re-arm a firing rule.
        assert_eq!(evaluate(&up, 50.0, now()).state_change(), StateChange::None);
    }

    #[test]
    fn non_finite_observation_is_skipped() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let decision = evaluate(&rule(Operator::Gt, 10.0), bad, now());
            assert!(matches!(decision, Decision::Skip { .. }), "{bad}");
            assert_eq!(decision.state_change(), StateChange::None);
        }
    }

    #[test]
    fn backwards_clock_does_not_defeat_the_cooldown() {
        let mut rule = rule(Operator::Gt, 10.0);
        // last_triggered_at in the future, as happens after an NTP step backwards.
        rule.last_triggered_at = Some(now() + Duration::seconds(120));

        assert!(matches!(
            evaluate(&rule, 11.0, now()),
            Decision::Suppressed { .. }
        ));
    }

    #[test]
    fn a_continuous_breach_notifies_exactly_once() {
        // Regression guard for the original defect: a rule that stays above its
        // threshold must produce one notification, not one per cooldown period.
        let mut rule = rule(Operator::Gt, 10.0);
        let mut notifications = 0;

        for minute in 0..120 {
            let at = now() + Duration::minutes(minute);
            let decision = evaluate(&rule, 999.0, at);

            if decision.should_notify() {
                notifications += 1;
                rule.last_triggered_at = Some(at);
            }

            match decision.state_change() {
                StateChange::ToFiring => rule.state = RuleState::Firing,
                StateChange::ToOk => rule.state = RuleState::Ok,
                StateChange::None => {}
            }
        }

        assert_eq!(notifications, 1);
    }
}
