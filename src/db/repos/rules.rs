use crate::db::Db;
use crate::error::{AppError, Result};
use crate::rules::types::{Operator, Rule, RuleRow, RuleState, TargetKind};
use chrono::{DateTime, Utc};

/// Every read resolves the rule together with its target in one statement. Loading
/// targets per rule would issue one query per rule on every scheduler tick.
const SELECT: &str = "\
    SELECT r.id, r.token_id, r.wallet_id, r.operator, r.threshold, r.cooldown_seconds, \
           r.reference_value, r.state, r.enabled, r.last_value, r.last_evaluated_at, \
           r.last_triggered_at, \
           t.mint_address AS mint_address, t.symbol AS symbol, \
           w.address AS wallet_address, w.label AS label \
    FROM rules r \
    LEFT JOIN tokens t ON t.id = r.token_id \
    LEFT JOIN wallets w ON w.id = r.wallet_id";

/// Where a new rule points. Mirrors the database's "exactly one target" invariant in
/// the type system so an impossible rule cannot be constructed in the first place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewRuleTarget {
    Token { id: i64 },
    Wallet { id: i64 },
}

impl NewRuleTarget {
    fn columns(self) -> (Option<i64>, Option<i64>) {
        match self {
            NewRuleTarget::Token { id } => (Some(id), None),
            NewRuleTarget::Wallet { id } => (None, Some(id)),
        }
    }

    pub fn kind(self) -> TargetKind {
        match self {
            NewRuleTarget::Token { .. } => TargetKind::Token,
            NewRuleTarget::Wallet { .. } => TargetKind::Wallet,
        }
    }
}

/// Values to persist after evaluating a rule.
#[derive(Debug, Clone, Copy)]
pub struct EvaluationOutcome {
    pub observed: f64,
    pub evaluated_at: DateTime<Utc>,
    /// `Some` only when the state machine transitions.
    pub state: Option<RuleState>,
    /// `Some` when a percentage baseline is set or re-armed.
    pub reference_value: Option<f64>,
    /// `Some` when an alert was actually delivered.
    pub triggered_at: Option<DateTime<Utc>>,
}

pub struct RuleRepo<'a> {
    db: &'a Db,
}

impl<'a> RuleRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn find(&self, id: i64) -> Result<Option<Rule>> {
        sqlx::query_as::<_, RuleRow>(&format!("{SELECT} WHERE r.id = ?"))
            .bind(id)
            .fetch_optional(self.db.pool())
            .await?
            .map(Rule::try_from)
            .transpose()
    }

    /// Rules the scheduler must evaluate, ordered so token lookups group together.
    pub async fn list_enabled(&self) -> Result<Vec<Rule>> {
        self.collect(&format!(
            "{SELECT} WHERE r.enabled = 1 ORDER BY r.token_id, r.wallet_id, r.id"
        ))
        .await
    }

    pub async fn list_all(&self) -> Result<Vec<Rule>> {
        self.collect(&format!("{SELECT} ORDER BY r.id")).await
    }

    async fn collect(&self, sql: &str) -> Result<Vec<Rule>> {
        sqlx::query_as::<_, RuleRow>(sql)
            .fetch_all(self.db.pool())
            .await?
            .into_iter()
            .map(Rule::try_from)
            .collect()
    }

    pub async fn create(
        &self,
        target: NewRuleTarget,
        operator: Operator,
        threshold: f64,
        cooldown_seconds: i64,
    ) -> Result<Rule> {
        if !threshold.is_finite() || threshold <= 0.0 {
            return Err(AppError::InvalidInput(
                "threshold must be a positive number".into(),
            ));
        }

        if cooldown_seconds < 0 {
            return Err(AppError::InvalidInput(
                "cooldown must not be negative".into(),
            ));
        }

        let (token_id, wallet_id) = target.columns();

        let result = sqlx::query(
            "INSERT INTO rules (token_id, wallet_id, operator, threshold, cooldown_seconds) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(token_id)
        .bind(wallet_id)
        .bind(operator.as_str())
        .bind(threshold)
        .bind(cooldown_seconds)
        .execute(self.db.pool())
        .await;

        match result {
            Ok(result) => self
                .find(result.last_insert_rowid())
                .await?
                .ok_or_else(|| AppError::Data("rule vanished immediately after insert".into())),
            // The partial unique indexes turn a duplicate rule into a clear conflict
            // instead of a second alert stream for the same condition.
            Err(err) if is_unique_violation(&err) => Err(AppError::Conflict(
                "an identical alert rule already exists for this target".into(),
            )),
            // The target was deleted between being chosen and the rule being saved.
            Err(err) if is_foreign_key_violation(&err) => Err(AppError::Conflict(
                "that target is no longer tracked".into(),
            )),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn set_enabled(&self, id: i64, enabled: bool) -> Result<Rule> {
        // Enabling resets all evaluation state, so "enable" means a clean start and
        // nothing else. Without this a rule re-enabled inside its old cooldown window
        // would latch straight back to firing and swallow the alert the user just
        // asked for, and a percentage rule would measure change across the entire
        // period it was switched off.
        let result = sqlx::query(
            "UPDATE rules SET enabled = ?1, \
                 state = 'ok', \
                 reference_value = CASE WHEN ?1 THEN NULL ELSE reference_value END, \
                 last_triggered_at = CASE WHEN ?1 THEN NULL ELSE last_triggered_at END, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ?2",
        )
        .bind(enabled)
        .bind(id)
        .execute(self.db.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("alert rule {id}")));
        }

        self.find(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("alert rule {id}")))
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let result = sqlx::query("DELETE FROM rules WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("alert rule {id}")));
        }

        Ok(())
    }

    /// Persists the result of one evaluation in a single statement, so a rule's
    /// observed value, state, baseline, and trigger time can never diverge.
    pub async fn record_evaluation(&self, id: i64, outcome: EvaluationOutcome) -> Result<()> {
        sqlx::query(
            "UPDATE rules SET \
                 last_value = ?1, \
                 last_evaluated_at = ?2, \
                 state = COALESCE(?3, state), \
                 reference_value = COALESCE(?4, reference_value), \
                 last_triggered_at = COALESCE(?5, last_triggered_at), \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ?6",
        )
        .bind(outcome.observed)
        .bind(outcome.evaluated_at)
        .bind(outcome.state.map(RuleState::as_str))
        .bind(outcome.reference_value)
        .bind(outcome.triggered_at)
        .bind(id)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    pub async fn count_enabled(&self) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM rules WHERE enabled = 1")
            .fetch_one(self.db.pool())
            .await?;
        Ok(count)
    }

    pub async fn count_all(&self) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM rules")
            .fetch_one(self.db.pool())
            .await?;
        Ok(count)
    }
}

pub(crate) fn is_unique_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("2067")
        || db.message().contains("UNIQUE constraint failed"))
}

fn is_foreign_key_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("787")
        || db.message().contains("FOREIGN KEY constraint failed"))
}
