//! The rule domain model.
//!
//! Values coming out of SQLite are parsed into closed enums at the repository
//! boundary. The previous model kept `kind`, `operator`, and `enabled` as raw
//! `String`/`i64` fields and coerced unknown values with `unwrap_or`, so a corrupted
//! operator silently became `>` and quietly produced wrong alerts. Anything the
//! database holds that this build cannot interpret is now a hard error instead.

use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Gt,
    Lt,
    Gte,
    Lte,
    /// Rose by at least `threshold` percent from the rolling baseline.
    PctUp,
    /// Fell by at least `threshold` percent from the rolling baseline.
    PctDown,
}

impl Operator {
    /// Canonical storage form. Matches the `CHECK` constraint on `rules.operator`.
    pub fn as_str(self) -> &'static str {
        match self {
            Operator::Gt => "gt",
            Operator::Lt => "lt",
            Operator::Gte => "gte",
            Operator::Lte => "lte",
            Operator::PctUp => "pct_up",
            Operator::PctDown => "pct_down",
        }
    }

    /// How the operator is written in chat.
    pub fn symbol(self) -> &'static str {
        match self {
            Operator::Gt => ">",
            Operator::Lt => "<",
            Operator::Gte => ">=",
            Operator::Lte => "<=",
            Operator::PctUp => "%up",
            Operator::PctDown => "%down",
        }
    }

    /// Accepts both the storage form and the symbols users type.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "gt" | ">" => Some(Operator::Gt),
            "lt" | "<" => Some(Operator::Lt),
            "gte" | ">=" => Some(Operator::Gte),
            "lte" | "<=" => Some(Operator::Lte),
            "pct_up" | "%up" | "up" => Some(Operator::PctUp),
            "pct_down" | "%down" | "down" => Some(Operator::PctDown),
            _ => None,
        }
    }

    /// Percentage operators compare against a moving baseline rather than an
    /// absolute value, which changes how they are stored and re-armed.
    pub fn is_percentage(self) -> bool {
        matches!(self, Operator::PctUp | Operator::PctDown)
    }

    pub fn all() -> [Operator; 6] {
        [
            Operator::Gt,
            Operator::Lt,
            Operator::Gte,
            Operator::Lte,
            Operator::PctUp,
            Operator::PctDown,
        ]
    }
}

fn parse_stored<T>(field: &'static str, raw: &str, parsed: Option<T>) -> Result<T> {
    parsed.ok_or_else(|| AppError::Data(format!("unsupported {field} `{raw}`")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetKind {
    /// A tracked SPL token; the observed value is its USD price.
    Token,
    /// A tracked wallet; the observed value is its native SOL balance.
    Wallet,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetKind::Token => "token",
            TargetKind::Wallet => "wallet",
        }
    }

    /// Unit of the observed value, used in every rendered message.
    pub fn unit(self) -> &'static str {
        match self {
            TargetKind::Token => "USD",
            TargetKind::Wallet => "SOL",
        }
    }

    pub fn metric(self) -> &'static str {
        match self {
            TargetKind::Token => "price",
            TargetKind::Wallet => "balance",
        }
    }
}

/// The thing a rule watches, resolved together with the rule so rendering and
/// alerting never need a second query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleTarget {
    pub kind: TargetKind,
    /// Primary key in `tokens` or `wallets`.
    pub id: i64,
    /// Mint address or wallet address.
    pub reference: String,
    /// Token symbol or wallet label, when the user supplied one.
    pub label: Option<String>,
}

impl RuleTarget {
    /// Short, human-first identification: the label when known, otherwise a
    /// truncated address (full base58 addresses make chat output unreadable).
    pub fn display(&self) -> String {
        match &self.label {
            Some(label) => format!("{label} ({})", abbreviate(&self.reference)),
            None => abbreviate(&self.reference),
        }
    }
}

/// Shortens a base58 address to a recognisable, copy-verifiable form.
pub fn abbreviate(address: &str) -> String {
    let chars: Vec<char> = address.chars().collect();
    if chars.len() <= 12 {
        return address.to_string();
    }

    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

/// Whether the rule's condition is currently held to be true.
///
/// This is what makes alerting edge-triggered: a rule fires on the transition into
/// `Firing` and stays quiet until the condition clears. Previously a rule whose
/// condition simply remained true re-notified on every cooldown expiry, forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleState {
    Ok,
    Firing,
}

impl RuleState {
    pub fn as_str(self) -> &'static str {
        match self {
            RuleState::Ok => "ok",
            RuleState::Firing => "firing",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "ok" => Some(RuleState::Ok),
            "firing" => Some(RuleState::Firing),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: i64,
    pub target: RuleTarget,
    pub operator: Operator,
    pub threshold: f64,
    pub cooldown_seconds: i64,
    /// Baseline for percentage operators. `None` until the first observation.
    pub reference_value: Option<f64>,
    pub state: RuleState,
    pub enabled: bool,
    pub last_value: Option<f64>,
    pub last_evaluated_at: Option<DateTime<Utc>>,
    pub last_triggered_at: Option<DateTime<Utc>>,
}

impl Rule {
    /// One-line description, e.g. `SOL balance < 5 SOL`.
    pub fn condition(&self) -> String {
        let unit = self.target.kind.unit();
        if self.operator.is_percentage() {
            format!(
                "{} {} {}%",
                self.target.kind.metric(),
                self.operator.symbol(),
                crate::alerts::format::amount(self.threshold)
            )
        } else {
            format!(
                "{} {} {} {}",
                self.target.kind.metric(),
                self.operator.symbol(),
                crate::alerts::format::amount(self.threshold),
                unit
            )
        }
    }
}

/// Raw `rules` row joined with its target. Private to the domain: the only way to
/// obtain a [`Rule`] is through validation.
#[derive(Debug, sqlx::FromRow)]
pub struct RuleRow {
    pub id: i64,
    pub token_id: Option<i64>,
    pub wallet_id: Option<i64>,
    pub operator: String,
    pub threshold: f64,
    pub cooldown_seconds: i64,
    pub reference_value: Option<f64>,
    pub state: String,
    pub enabled: i64,
    pub last_value: Option<f64>,
    pub last_evaluated_at: Option<DateTime<Utc>>,
    pub last_triggered_at: Option<DateTime<Utc>>,
    pub mint_address: Option<String>,
    pub symbol: Option<String>,
    pub wallet_address: Option<String>,
    pub label: Option<String>,
}

impl TryFrom<RuleRow> for Rule {
    type Error = AppError;

    fn try_from(row: RuleRow) -> Result<Self> {
        let target = match (row.token_id, row.wallet_id) {
            (Some(id), None) => RuleTarget {
                kind: TargetKind::Token,
                id,
                reference: row.mint_address.ok_or_else(|| {
                    AppError::Data(format!("rule {} references a missing token", row.id))
                })?,
                label: row.symbol,
            },
            (None, Some(id)) => RuleTarget {
                kind: TargetKind::Wallet,
                id,
                reference: row.wallet_address.ok_or_else(|| {
                    AppError::Data(format!("rule {} references a missing wallet", row.id))
                })?,
                label: row.label,
            },
            _ => {
                return Err(AppError::Data(format!(
                    "rule {} must target exactly one token or wallet",
                    row.id
                )))
            }
        };

        Ok(Rule {
            id: row.id,
            target,
            operator: parse_stored("operator", &row.operator, Operator::parse(&row.operator))?,
            threshold: row.threshold,
            cooldown_seconds: row.cooldown_seconds,
            reference_value: row.reference_value,
            state: parse_stored("rule state", &row.state, RuleState::parse(&row.state))?,
            enabled: row.enabled != 0,
            last_value: row.last_value,
            last_evaluated_at: row.last_evaluated_at,
            last_triggered_at: row.last_triggered_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_round_trips_through_storage_and_symbols() {
        for op in Operator::all() {
            assert_eq!(
                Operator::parse(op.as_str()),
                Some(op),
                "{op:?} storage form"
            );
            assert_eq!(Operator::parse(op.symbol()), Some(op), "{op:?} symbol");
        }
    }

    #[test]
    fn operator_parsing_is_case_and_space_insensitive() {
        assert_eq!(Operator::parse(" %UP "), Some(Operator::PctUp));
        assert_eq!(Operator::parse("PCT_DOWN"), Some(Operator::PctDown));
        assert_eq!(Operator::parse("≥"), None);
        assert_eq!(Operator::parse(""), None);
    }

    fn row() -> RuleRow {
        RuleRow {
            id: 1,
            token_id: Some(7),
            wallet_id: None,
            operator: "gt".into(),
            threshold: 1.5,
            cooldown_seconds: 300,
            reference_value: None,
            state: "ok".into(),
            enabled: 1,
            last_value: None,
            last_evaluated_at: None,
            last_triggered_at: None,
            mint_address: Some("MINT".into()),
            symbol: Some("USDC".into()),
            wallet_address: None,
            label: None,
        }
    }

    #[test]
    fn valid_row_becomes_a_rule() {
        let rule = Rule::try_from(row()).unwrap();
        assert_eq!(rule.target.kind, TargetKind::Token);
        assert_eq!(rule.target.id, 7);
        assert_eq!(rule.operator, Operator::Gt);
        assert!(rule.enabled);
    }

    #[test]
    fn unknown_operator_is_an_error_not_a_silent_default() {
        let bad = RuleRow {
            operator: "approximately".into(),
            ..row()
        };
        let err = Rule::try_from(bad).unwrap_err();
        assert!(matches!(err, AppError::Data(_)), "{err}");
    }

    #[test]
    fn unknown_state_is_an_error() {
        let bad = RuleRow {
            state: "wobbling".into(),
            ..row()
        };
        assert!(matches!(Rule::try_from(bad), Err(AppError::Data(_))));
    }

    #[test]
    fn ambiguous_or_missing_target_is_rejected() {
        let both = RuleRow {
            wallet_id: Some(9),
            wallet_address: Some("W".into()),
            ..row()
        };
        assert!(matches!(Rule::try_from(both), Err(AppError::Data(_))));

        let neither = RuleRow {
            token_id: None,
            ..row()
        };
        assert!(matches!(Rule::try_from(neither), Err(AppError::Data(_))));

        let dangling = RuleRow {
            mint_address: None,
            ..row()
        };
        assert!(matches!(Rule::try_from(dangling), Err(AppError::Data(_))));
    }

    #[test]
    fn abbreviates_long_addresses_only() {
        assert_eq!(abbreviate("short"), "short");
        assert_eq!(
            abbreviate("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
            "EPjF…Dt1v"
        );
    }

    #[test]
    fn target_display_prefers_label() {
        let target = RuleTarget {
            kind: TargetKind::Token,
            id: 1,
            reference: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            label: Some("USDC".into()),
        };
        assert_eq!(target.display(), "USDC (EPjF…Dt1v)");

        let unlabelled = RuleTarget {
            label: None,
            ..target
        };
        assert_eq!(unlabelled.display(), "EPjF…Dt1v");
    }
}
