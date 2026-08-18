use chainsentinel::rules::types::{Operator, Rule, RuleKind, RuleOutcome, Sample, TargetType};
use chainsentinel::rules::evaluate;
use chrono::Utc;

fn rule(operator: Operator, threshold: f64, reference: Option<f64>) -> Rule {
    Rule {
        id: 1,
        kind: RuleKind::Price.as_str().into(),
        target_type: TargetType::Token.as_str().into(),
        target_ref: "mint".into(),
        metric: "price".into(),
        operator: operator.as_str().into(),
        threshold,
        time_window_seconds: None,
        cooldown_seconds: 300,
        max_triggers: None,
        reference_value: reference,
        enabled: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        deleted_at: None,
    }
}

#[test]
fn threshold_operators_work() {
    assert!(matches!(
        evaluate(&rule(Operator::Gt, 10.0, None), &Sample { value: 11.0, reference: None }),
        RuleOutcome::Trigger { .. }
    ));

    assert_eq!(
        evaluate(&rule(Operator::Gt, 10.0, None), &Sample { value: 10.0, reference: None }),
        RuleOutcome::NoTrigger
    );

    assert!(matches!(
        evaluate(&rule(Operator::Gte, 10.0, None), &Sample { value: 10.0, reference: None }),
        RuleOutcome::Trigger { .. }
    ));

    assert!(matches!(
        evaluate(&rule(Operator::Lt, 10.0, None), &Sample { value: 9.0, reference: None }),
        RuleOutcome::Trigger { .. }
    ));

    assert!(matches!(
        evaluate(&rule(Operator::Lte, 10.0, None), &Sample { value: 10.0, reference: None }),
        RuleOutcome::Trigger { .. }
    ));
}

#[test]
fn percentage_operators_compute_change() {
    assert!(matches!(
        evaluate(&rule(Operator::PctChangeUp, 10.0, Some(100.0)), &Sample { value: 115.0, reference: Some(100.0) }),
        RuleOutcome::Trigger { current, .. } if current == 15.0
    ));

    assert!(matches!(
        evaluate(&rule(Operator::PctChangeDown, 10.0, Some(100.0)), &Sample { value: 85.0, reference: Some(100.0) }),
        RuleOutcome::Trigger { .. }
    ));

    assert_eq!(
        evaluate(&rule(Operator::PctChangeUp, 20.0, Some(100.0)), &Sample { value: 110.0, reference: Some(100.0) }),
        RuleOutcome::NoTrigger
    );
}
