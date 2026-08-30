//! Rendering of values, conditions, and alert messages.
//!
//! This is the single vocabulary the whole application speaks about a rule. Screens,
//! history, command replies, and delivered alerts all render a threshold through
//! [`valued`] and a condition through [`condition`], so "at or below $104.8" reads the
//! same everywhere instead of appearing as `<= 104.8 USD` on one surface and
//! `at or below $104.8` on another.
//!
//! Alert bodies are plain text. Telegram's Markdown and HTML parse modes are
//! deliberately not used here because an alert carries user-supplied labels: a single
//! unescaped character would fail the send, and a failed send is a missed alert. Screens
//! can afford HTML because a failed screen is a retry, not a lost notification.

use crate::rules::eval::Decision;
use crate::rules::types::{Operator, Rule, TargetKind};
use chrono::{DateTime, Utc};

/// Formats a quantity with enough precision to stay meaningful, including sub-cent
/// token prices.
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

/// Percentage change of `observed` relative to `baseline`.
pub fn change_pct(observed: f64, baseline: f64) -> Option<f64> {
    if baseline == 0.0 || !baseline.is_finite() || !observed.is_finite() {
        return None;
    }

    Some((observed - baseline) / baseline.abs() * 100.0)
}

/// A move, worded rather than signed: `down 15%` / `up 15%`.
///
/// A leading `-` is easy to miss in a notification, and "down" cannot be misread.
pub fn moved(pct: f64) -> String {
    if !pct.is_finite() {
        return "n/a".to_string();
    }

    format!(
        "{} {}%",
        if pct < 0.0 { "down" } else { "up" },
        amount(pct.abs())
    )
}

/// A value carrying its unit, e.g. `$0.0000025` or `2 SOL`.
///
/// The unit is attached where each unit is conventionally written — `$` leads, `SOL`
/// trails — so a reader never has to work out which number is money.
pub fn valued(kind: TargetKind, value: f64) -> String {
    match kind {
        TargetKind::Token => format!("${}", amount(value)),
        TargetKind::Wallet => format!("{} SOL", amount(value)),
    }
}

/// A rule's condition in plain language, e.g. `at or below $104.8` or `down 10%`.
///
/// Takes the parts rather than a [`Rule`] so the guided flow can render the condition
/// it is about to create, before any rule exists.
pub fn condition(kind: TargetKind, operator: Operator, threshold: f64) -> String {
    match operator {
        Operator::Gt => format!("above {}", valued(kind, threshold)),
        Operator::Lt => format!("below {}", valued(kind, threshold)),
        Operator::Gte => format!("at or above {}", valued(kind, threshold)),
        Operator::Lte => format!("at or below {}", valued(kind, threshold)),
        Operator::PctUp => format!("up {}%", amount(threshold)),
        Operator::PctDown => format!("down {}%", amount(threshold)),
    }
}

/// The glyph for an operator.
///
/// These are the same six glyphs the condition buttons carry, so a delivered alert is
/// marked with the button the user tapped to create it.
pub fn operator_glyph(operator: Operator) -> &'static str {
    match operator {
        Operator::Gt | Operator::Gte => "⬆️",
        Operator::Lt | Operator::Lte => "⬇️",
        Operator::PctUp => "📈",
        Operator::PctDown => "📉",
    }
}

/// Full precision, for detail screens and `/status` where the exact second is
/// operationally useful.
pub fn timestamp(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Minute precision, for alert footers.
///
/// Polls are at least ten seconds apart and a rule fires once per crossing, so the
/// seconds distinguish nothing and only lengthen the line.
pub fn timestamp_short(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%d %H:%M UTC").to_string()
}

/// Renders the alert delivered to admins.
///
/// The shape is deliberately a headline, then only what the headline does not already
/// say, then one footer:
///
/// ```text
/// ⬇️ SOL is at or below $104.8
///
/// Price now: $104.76
/// 2026-08-30 09:26 UTC · alert #2
/// ```
///
/// The target is named once. Its address is not repeated alongside a label it already
/// resolves to, and appears only when there is no label — in which case the name *is*
/// the abbreviated address.
pub fn alert_message(rule: &Rule, decision: &Decision, at: DateTime<Utc>) -> Option<String> {
    let Decision::Notify {
        observed,
        reference,
    } = decision
    else {
        return None;
    };

    let kind = rule.target.kind;
    let name = rule.target.name();
    let glyph = operator_glyph(rule.operator);
    let metric = capitalized(kind.metric());

    let mut lines = Vec::with_capacity(5);

    if rule.operator.is_percentage() {
        // The news is the move that happened; the rule is only why we are saying so.
        // A baseline is always stored before a percentage rule can fire, but if one is
        // somehow missing the rule's own wording still describes the situation.
        let moved_pct = reference.and_then(|baseline| change_pct(*observed, baseline));

        match moved_pct {
            Some(pct) => lines.push(format!("{glyph} {name} is {}", moved(pct))),
            None => lines.push(format!(
                "{glyph} {name} is {}",
                condition(kind, rule.operator, rule.threshold)
            )),
        }

        lines.push(String::new());

        match reference {
            Some(baseline) => lines.push(format!(
                "{metric} now: {}, from {}",
                valued(kind, *observed),
                valued(kind, *baseline)
            )),
            None => lines.push(format!("{metric} now: {}", valued(kind, *observed))),
        }

        // Restated because the observed move is larger than the threshold, so without
        // this the reader cannot tell which of their alerts just fired.
        lines.push(format!(
            "Alert fires on a {}% {}",
            amount(rule.threshold),
            if rule.operator == Operator::PctUp {
                "rise"
            } else {
                "drop"
            }
        ));
    } else {
        lines.push(format!(
            "{glyph} {name} is {}",
            condition(kind, rule.operator, rule.threshold)
        ));
        lines.push(String::new());
        lines.push(format!("{metric} now: {}", valued(kind, *observed)));
    }

    // The id is what `/deleterule` and `/disablerule` take, and "alert" is the word the
    // rest of the interface uses for a rule.
    lines.push(format!("{} · alert #{}", timestamp_short(at), rule.id));

    Some(lines.join("\n"))
}

/// `Price` / `Balance`, for use at the start of a line.
fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::types::{RuleState, RuleTarget};

    /// The rule behind the message the redesign was judged against: a `<=` price alert
    /// on SOL.
    fn rule(operator: Operator, threshold: f64, kind: TargetKind) -> Rule {
        Rule {
            id: 12,
            target: RuleTarget {
                kind,
                id: 1,
                reference: "So11111111111111111111111111111111111111112".into(),
                label: Some("SOL".into()),
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
        // Small token prices must retain enough precision to remain useful.
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
        assert_eq!(moved(f64::NAN), "n/a");
    }

    #[test]
    fn a_move_is_worded_rather_than_signed() {
        // A leading minus is easy to miss in a push notification; "down" is not.
        assert_eq!(moved(12.5), "up 12.5%");
        assert_eq!(moved(-3.0), "down 3%");
        assert_eq!(moved(0.0), "up 0%");
    }

    #[test]
    fn change_pct_guards_against_a_zero_baseline() {
        assert_eq!(change_pct(10.0, 0.0), None);
        assert_eq!(change_pct(110.0, 100.0), Some(10.0));
        assert_eq!(change_pct(90.0, 100.0), Some(-10.0));
    }

    #[test]
    fn a_value_carries_its_unit_where_that_unit_is_written() {
        assert_eq!(valued(TargetKind::Token, 104.76), "$104.76");
        assert_eq!(valued(TargetKind::Wallet, 4.25), "4.25 SOL");
    }

    #[test]
    fn conditions_read_as_words_not_operators() {
        // The same six phrases the condition buttons offer, so an alert describes the
        // rule using the words the user chose it by.
        let cases = [
            (Operator::Gt, "above $2"),
            (Operator::Lt, "below $2"),
            (Operator::Gte, "at or above $2"),
            (Operator::Lte, "at or below $2"),
            (Operator::PctUp, "up 2%"),
            (Operator::PctDown, "down 2%"),
        ];

        for (operator, expected) in cases {
            assert_eq!(condition(TargetKind::Token, operator, 2.0), expected);
        }

        assert_eq!(
            condition(TargetKind::Wallet, Operator::Lte, 5.0),
            "at or below 5 SOL"
        );
    }

    #[test]
    fn every_operator_has_a_glyph_matching_its_button() {
        for operator in Operator::all() {
            assert!(!operator_glyph(operator).is_empty(), "{operator:?}");
        }
        assert_eq!(operator_glyph(Operator::Gt), operator_glyph(Operator::Gte));
        assert_ne!(operator_glyph(Operator::Gt), operator_glyph(Operator::Lt));
        assert_ne!(
            operator_glyph(Operator::PctUp),
            operator_glyph(Operator::PctDown)
        );
    }

    #[test]
    fn alert_footers_drop_the_second_that_distinguishes_nothing() {
        assert_eq!(timestamp(at()), "2026-01-01 12:00:00 UTC");
        assert_eq!(timestamp_short(at()), "2026-01-01 12:00 UTC");
    }

    #[test]
    fn a_threshold_alert_leads_with_what_happened() {
        let message = alert_message(
            &rule(Operator::Lte, 104.8, TargetKind::Token),
            &Decision::Notify {
                observed: 104.76,
                reference: Some(104.8),
            },
            at(),
        )
        .unwrap();

        // The headline is a sentence naming the target and the condition it just met.
        assert_eq!(
            message.lines().next().unwrap(),
            "⬇️ SOL is at or below $104.8"
        );
        assert!(message.contains("Price now: $104.76"), "{message}");
        assert!(message.contains("alert #12"), "{message}");
        assert!(message.contains("2026-01-01 12:00 UTC"), "{message}");
    }

    #[test]
    fn an_alert_names_its_target_exactly_once() {
        // The old format printed "SOL (So11…1112)" and then restated the threshold in a
        // second notation, which is what made it dense and hard to skim.
        let message = alert_message(
            &rule(Operator::Lte, 104.8, TargetKind::Token),
            &Decision::Notify {
                observed: 104.76,
                reference: Some(104.8),
            },
            at(),
        )
        .unwrap();

        assert_eq!(message.matches("SOL").count(), 1, "{message}");
        // A label resolves the address, so repeating the address is noise.
        assert!(!message.contains("So11"), "{message}");
        // No operator symbols, no bare unit codes.
        for jargon in ["<=", "Rule:", "Rule id", "USD", "Target:"] {
            assert!(!message.contains(jargon), "{jargon} in: {message}");
        }
    }

    #[test]
    fn an_unlabelled_target_is_identified_by_its_address() {
        let mut rule = rule(Operator::Gt, 1.0, TargetKind::Token);
        rule.target.label = None;

        let message = alert_message(
            &rule,
            &Decision::Notify {
                observed: 2.0,
                reference: Some(1.0),
            },
            at(),
        )
        .unwrap();

        // With no label there is nothing else to call it, so the abbreviated address is
        // the name rather than an extra line.
        assert!(message.starts_with("⬆️ So11…1112 is above $1"), "{message}");
    }

    #[test]
    fn a_percentage_alert_leads_with_the_move_that_happened() {
        let message = alert_message(
            &rule(Operator::PctDown, 10.0, TargetKind::Token),
            &Decision::Notify {
                observed: 85.0,
                reference: Some(100.0),
            },
            at(),
        )
        .unwrap();

        // The observed move, not the threshold, is the news.
        assert_eq!(message.lines().next().unwrap(), "📉 SOL is down 15%");
        assert!(message.contains("Price now: $85, from $100"), "{message}");
        // The threshold still appears, because -15% does not say which alert fired.
        assert!(message.contains("Alert fires on a 10% drop"), "{message}");
    }

    #[test]
    fn a_percentage_alert_without_a_baseline_still_describes_the_rule() {
        // Defensive: a baseline is always stored before a percentage rule can fire.
        let message = alert_message(
            &rule(Operator::PctUp, 10.0, TargetKind::Token),
            &Decision::Notify {
                observed: 85.0,
                reference: None,
            },
            at(),
        )
        .unwrap();

        assert!(message.starts_with("📈 SOL is up 10%"), "{message}");
        assert!(message.contains("Price now: $85"), "{message}");
        assert!(!message.contains("from"), "{message}");
    }

    #[test]
    fn wallet_alerts_talk_about_a_balance_in_sol() {
        let message = alert_message(
            &rule(Operator::Lte, 5.0, TargetKind::Wallet),
            &Decision::Notify {
                observed: 4.25,
                reference: Some(5.0),
            },
            at(),
        )
        .unwrap();

        assert!(message.contains("is at or below 5 SOL"), "{message}");
        assert!(message.contains("Balance now: 4.25 SOL"), "{message}");
    }

    #[test]
    fn an_alert_is_short_enough_to_read_without_scrolling() {
        // Density was the original complaint: an alert arrives as a push notification,
        // so it has to be skimmable at a glance.
        for operator in Operator::all() {
            for kind in [TargetKind::Token, TargetKind::Wallet] {
                let message = alert_message(
                    &rule(operator, 10.0, kind),
                    &Decision::Notify {
                        observed: 85.0,
                        reference: Some(100.0),
                    },
                    at(),
                )
                .unwrap();

                let lines = message.lines().count();
                assert!(lines <= 5, "{operator:?}/{kind:?} is {lines} lines");
                for line in message.lines() {
                    assert!(
                        line.chars().count() <= 60,
                        "{operator:?}/{kind:?}: {line:?}"
                    );
                }
                // A blank line separating the headline from the detail is what makes it
                // scannable rather than a wall.
                assert!(message.contains("\n\n"), "{message}");
                assert!(!message.ends_with('\n'), "{message}");
            }
        }
    }

    #[test]
    fn a_label_that_looks_like_markup_is_delivered_literally() {
        // Alerts are sent without a parse mode precisely so a hostile label cannot
        // break the send. Nothing may be escaped or stripped on the way out.
        let mut rule = rule(Operator::Gt, 1.0, TargetKind::Token);
        rule.target.label = Some("<b>&amp;</b>".into());

        let message = alert_message(
            &rule,
            &Decision::Notify {
                observed: 2.0,
                reference: Some(1.0),
            },
            at(),
        )
        .unwrap();

        assert!(message.contains("<b>&amp;</b> is above $1"), "{message}");
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
