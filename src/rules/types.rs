use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Price,
    Balance,
}

impl RuleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RuleKind::Price => "price",
            RuleKind::Balance => "balance",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "price" => Ok(RuleKind::Price),
            "balance" => Ok(RuleKind::Balance),
            other => Err(format!("unknown rule kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetType {
    Token,
    Wallet,
}

impl TargetType {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetType::Token => "token",
            TargetType::Wallet => "wallet",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "token" => Ok(TargetType::Token),
            "wallet" => Ok(TargetType::Wallet),
            other => Err(format!("unknown target type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Gt,
    Lt,
    Gte,
    Lte,
    PctChangeUp,
    PctChangeDown,
}

impl Operator {
    pub fn as_str(self) -> &'static str {
        match self {
            Operator::Gt => ">",
            Operator::Lt => "<",
            Operator::Gte => ">=",
            Operator::Lte => "<=",
            Operator::PctChangeUp => "pct_change_up",
            Operator::PctChangeDown => "pct_change_down",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            ">" | "gt" => Ok(Operator::Gt),
            "<" | "lt" => Ok(Operator::Lt),
            ">=" | "gte" => Ok(Operator::Gte),
            "<=" | "lte" => Ok(Operator::Lte),
            "pct_change_up" | "%up" => Ok(Operator::PctChangeUp),
            "pct_change_down" | "%down" => Ok(Operator::PctChangeDown),
            other => Err(format!("unknown operator: {other}")),
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct Rule {
    pub id: i64,
    pub kind: String,
    pub target_type: String,
    pub target_ref: String,
    pub metric: String,
    pub operator: String,
    pub threshold: f64,
    pub time_window_seconds: Option<i64>,
    pub cooldown_seconds: i64,
    pub max_triggers: Option<i64>,
    pub reference_value: Option<f64>,
    pub enabled: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Rule {
    pub fn kind(&self) -> RuleKind {
        RuleKind::parse(&self.kind).unwrap_or(RuleKind::Price)
    }

    pub fn target_type(&self) -> TargetType {
        TargetType::parse(&self.target_type).unwrap_or(TargetType::Token)
    }

    pub fn operator(&self) -> Operator {
        Operator::parse(&self.operator).unwrap_or(Operator::Gt)
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled != 0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub value: f64,
    pub reference: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RuleOutcome {
    NoTrigger,
    Trigger { current: f64, threshold: f64 },
}
