use crate::rules::types::{Operator, Rule, RuleOutcome, Sample};

pub fn evaluate(rule: &Rule, sample: &Sample) -> RuleOutcome {
    match rule.operator() {
        Operator::Gt | Operator::Lt | Operator::Gte | Operator::Lte => {
            compare_threshold(rule.operator(), sample.value, rule.threshold)
        }
        Operator::PctChangeUp | Operator::PctChangeDown => {
            let reference = match sample.reference {
                Some(reference) if reference != 0.0 => reference,
                _ => return RuleOutcome::NoTrigger,
            };

            let pct_change = (sample.value - reference) / reference * 100.0;

            match rule.operator() {
                Operator::PctChangeUp => {
                    compare_threshold(Operator::Gte, pct_change, rule.threshold)
                }
                Operator::PctChangeDown => {
                    compare_threshold(Operator::Lte, pct_change, -rule.threshold)
                }
                _ => unreachable!(),
            }
        }
    }
}

fn compare_threshold(operator: Operator, value: f64, threshold: f64) -> RuleOutcome {
    let triggered = match operator {
        Operator::Gt => value > threshold,
        Operator::Lt => value < threshold,
        Operator::Gte => value >= threshold,
        Operator::Lte => value <= threshold,
        _ => false,
    };

    if triggered {
        RuleOutcome::Trigger {
            current: value,
            threshold,
        }
    } else {
        RuleOutcome::NoTrigger
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::types::{RuleKind, TargetType};
    use chrono::Utc;

    fn base_rule(operator: &str, threshold: f64) -> Rule {
        Rule {
            id: 1,
            kind: RuleKind::Price.as_str().to_string(),
            target_type: TargetType::Token.as_str().to_string(),
            target_ref: "mint".into(),
            metric: "price".into(),
            operator: operator.into(),
            threshold,
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

    #[test]
    fn gt_triggers_above_threshold() {
        let rule = base_rule(">", 100.0);
        let sample = Sample {
            value: 101.0,
            reference: None,
        };
        assert!(
            matches!(evaluate(&rule, &sample), RuleOutcome::Trigger { current, threshold } if current == 101.0 && threshold == 100.0)
        );
    }

    #[test]
    fn lt_triggers_below_threshold() {
        let rule = base_rule("<", 100.0);
        let sample = Sample {
            value: 99.0,
            reference: None,
        };
        assert!(matches!(
            evaluate(&rule, &sample),
            RuleOutcome::Trigger { .. }
        ));
    }

    #[test]
    fn pct_change_up_uses_reference() {
        let rule = base_rule("pct_change_up", 10.0);
        let sample = Sample {
            value: 121.0,
            reference: Some(110.0),
        };
        assert!(
            matches!(evaluate(&rule, &sample), RuleOutcome::Trigger { current, .. } if current == 10.0)
        );
    }

    #[test]
    fn pct_change_down_uses_negative_threshold() {
        let rule = base_rule("pct_change_down", 10.0);
        let sample = Sample {
            value: 90.0,
            reference: Some(110.0),
        };
        assert!(matches!(
            evaluate(&rule, &sample),
            RuleOutcome::Trigger { .. }
        ));
    }

    #[test]
    fn pct_change_without_reference_does_not_trigger() {
        let rule = base_rule("pct_change_up", 10.0);
        let sample = Sample {
            value: 200.0,
            reference: None,
        };
        assert_eq!(evaluate(&rule, &sample), RuleOutcome::NoTrigger);
    }
}
