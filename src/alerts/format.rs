//! Rendering of values and alert messages.
//!
//! All output is plain text. Telegram's Markdown parse modes are deliberately not
//! used: alert bodies contain user-supplied labels and base58 addresses, which would
//! need escaping on every path, and a single missed escape turns into a failed send
//! (and therefore a missed alert) rather than a cosmetic defect.

use crate::rules::eval::Decision;
use crate::rules::types::{Operator, Rule, TargetKind};
use chrono::{DateTime, Utc};

/// Formats a quantity with enough precision to stay meaningful.
///
/// Fixed two-decimal formatting was used everywhere previously, which rendered any
/// token priced below a cent as `0.00` — i.e. useless for most of what people
/// actually track on Solana, and actively misleading in an alert.
pub fn amount(value: f64) -> String {
    if !value.is_finite() {
        return "n/a".to_string();
    }

    let magnitude = value.abs();

    let decimals = if magnitude == 0.0 {
        2
    } else if magnitude >= 1.0 {
        4
    } else {
        // Keep roughly four significant digits below 1.
        (-magnitude.log10().floor()) as usize + 3
    };

    let rendered = format!("{value:.*}", decimals.min(12));

    if rendered.contains('.') {
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        rendered
    }
}

/// Formats a percentage change with an explicit sign.
pub fn percent(value: f64) -> String {
    if !value.is_finite() {
        return "n/a".to_string();
    }

    format!("{}{}%", if value >= 0.0 { "+" } else { "" }, amount(value))
}

/// Percentage change of `observed` relative to `baseline`.
pub fn change_pct(observed: f64, baseline: f64) -> Option<f64> {
    if baseline == 0.0 || !baseline.is_finite() || !observed.is_finite() {
        return None;
    }

    Some((observed - baseline) / baseline.abs() * 100.0)
}

pub fn timestamp(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Renders the alert delivered to admins.
pub fn alert_message(rule: &Rule, decision: &Decision, at: DateTime<Utc>) -> Option<String> {
    let Decision::Notify {
        observed,
        reference,
    } = decision
    else {
        return None;
    };

    let unit = rule.target.kind.unit();
    let subject = match rule.target.kind {
        TargetKind::Token => "Token price",
        TargetKind::Wallet => "Wallet balance",
    };

    let mut lines = vec![
        format!("\u{26a0}\u{fe0f} {subject} alert"),
        format!("Target: {}", rule.target.display()),
        format!("Now: {} {unit}", amount(*observed)),
    ];

    if rule.operator.is_percentage() {
        // Percentage rules previously reported the computed percentage in the
        // "current value" field, so the message showed neither the price nor the
        // baseline it was measured against.
        let baseline = reference.unwrap_or_default();
        let moved = change_pct(*observed, baseline)
            .map(percent)
            .unwrap_or_else(|| "n/a".to_string());

        lines.push(format!("Baseline: {} {unit}", amount(baseline)));
        lines.push(format!(
            "Change: {moved} (rule triggers at {}{}%)",
            if rule.operator == Operator::PctDown {
                "-"
            } else {
                "+"
            },
            amount(rule.threshold)
        ));
    } else {
        lines.push(format!(
            "Rule: {} {} {unit}",
            rule.operator.symbol(),
            amount(rule.threshold)
        ));
    }

    lines.push(format!("At: {}", timestamp(at)));
    lines.push(format!("Rule id: {}", rule.id));

    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::types::{RuleState, RuleTarget};

    fn rule(operator: Operator, threshold: f64, kind: TargetKind) -> Rule {
        Rule {
            id: 12,
            target: RuleTarget {
                kind,
                id: 1,
                reference: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
                label: Some("USDC".into()),
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

    fn at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn keeps_precision_for_sub_cent_prices() {
        // The original `{:.2}` rendering collapsed all of these to "0.00".
        assert_eq!(amount(0.0000123), "0.0000123");
        assert_eq!(amount(0.00000000456), "0.00000000456");
        assert_eq!(amount(0.999893), "0.9999");
    }

    #[test]
    fn trims_noise_from_round_numbers() {
        assert_eq!(amount(91.69), "91.69");
        assert_eq!(amount(1.0), "1");
        assert_eq!(amount(0.0), "0");
        assert_eq!(amount(1234.5), "1234.5");
        assert_eq!(amount(-2.5), "-2.5");
    }

    #[test]
    fn renders_non_finite_values_safely() {
        assert_eq!(amount(f64::NAN), "n/a");
        assert_eq!(amount(f64::INFINITY), "n/a");
        assert_eq!(percent(f64::NAN), "n/a");
    }

    #[test]
    fn percent_always_carries_a_sign() {
        assert_eq!(percent(12.5), "+12.5%");
        assert_eq!(percent(-3.0), "-3%");
        assert_eq!(percent(0.0), "+0%");
    }

    #[test]
    fn change_pct_guards_against_a_zero_baseline() {
        assert_eq!(change_pct(10.0, 0.0), None);
        assert_eq!(change_pct(110.0, 100.0), Some(10.0));
        assert_eq!(change_pct(90.0, 100.0), Some(-10.0));
    }

    #[test]
    fn threshold_alert_states_the_rule_and_the_reading() {
        let message = alert_message(
            &rule(Operator::Lt, 0.99, TargetKind::Token),
            &Decision::Notify {
                observed: 0.9812,
                reference: Some(0.99),
            },
            at(),
        )
        .unwrap();

        assert!(message.contains("Token price alert"), "{message}");
        assert!(message.contains("USDC (EPjF…Dt1v)"), "{message}");
        assert!(message.contains("Now: 0.9812 USD"), "{message}");
        assert!(message.contains("Rule: < 0.99 USD"), "{message}");
        assert!(message.contains("Rule id: 12"), "{message}");
    }

    #[test]
    fn percentage_alert_shows_price_baseline_and_move() {
        let message = alert_message(
            &rule(Operator::PctDown, 10.0, TargetKind::Token),
            &Decision::Notify {
                observed: 85.0,
                reference: Some(100.0),
            },
            at(),
        )
        .unwrap();

        assert!(message.contains("Now: 85 USD"), "{message}");
        assert!(message.contains("Baseline: 100 USD"), "{message}");
        assert!(message.contains("Change: -15%"), "{message}");
        assert!(message.contains("triggers at -10%"), "{message}");
    }

    #[test]
    fn wallet_alerts_are_denominated_in_sol() {
        let message = alert_message(
            &rule(Operator::Lte, 5.0, TargetKind::Wallet),
            &Decision::Notify {
                observed: 4.25,
                reference: Some(5.0),
            },
            at(),
        )
        .unwrap();

        assert!(message.contains("Wallet balance alert"), "{message}");
        assert!(message.contains("Now: 4.25 SOL"), "{message}");
    }

    #[test]
    fn non_notifying_decisions_render_nothing() {
        assert!(alert_message(
            &rule(Operator::Gt, 1.0, TargetKind::Token),
            &Decision::Clear,
            at()
        )
        .is_none());
    }
}
