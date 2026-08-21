use crate::db::Db;
use crate::error::Result;
use crate::rules::types::{Rule, TargetKind};
use chrono::{DateTime, Utc};
use sqlx::FromRow;

/// One delivered alert. Append-only: target details are snapshotted at trigger time
/// so history stays readable after the rule or its target is deleted, and so history
/// can be re-rendered rather than being frozen as a pre-formatted blob.
#[derive(Debug, Clone, FromRow)]
pub struct AlertEvent {
    pub id: i64,
    pub rule_id: Option<i64>,
    pub target_kind: String,
    pub target_ref: String,
    pub target_label: Option<String>,
    pub operator: String,
    pub threshold_value: f64,
    pub observed_value: f64,
    pub reference_value: Option<f64>,
    pub triggered_at: DateTime<Utc>,
}

impl AlertEvent {
    pub fn kind(&self) -> Option<TargetKind> {
        match self.target_kind.as_str() {
            "token" => Some(TargetKind::Token),
            "wallet" => Some(TargetKind::Wallet),
            _ => None,
        }
    }
}

pub struct AlertEventRepo<'a> {
    db: &'a Db,
}

impl<'a> AlertEventRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn record(
        &self,
        rule: &Rule,
        observed: f64,
        reference: Option<f64>,
        triggered_at: DateTime<Utc>,
    ) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO alert_events (\
                 rule_id, target_kind, target_ref, target_label, operator, \
                 threshold_value, observed_value, reference_value, triggered_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(rule.id)
        .bind(rule.target.kind.as_str())
        .bind(&rule.target.reference)
        .bind(rule.target.label.as_deref())
        .bind(rule.operator.as_str())
        .bind(rule.threshold)
        .bind(observed)
        .bind(reference)
        .bind(triggered_at)
        .execute(self.db.pool())
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn list_recent(&self, limit: i64) -> Result<Vec<AlertEvent>> {
        Ok(sqlx::query_as::<_, AlertEvent>(
            "SELECT id, rule_id, target_kind, target_ref, target_label, operator, \
                    threshold_value, observed_value, reference_value, triggered_at \
             FROM alert_events ORDER BY triggered_at DESC, id DESC LIMIT ?",
        )
        .bind(limit.clamp(1, 100))
        .fetch_all(self.db.pool())
        .await?)
    }

    pub async fn count(&self) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM alert_events")
            .fetch_one(self.db.pool())
            .await?;
        Ok(count)
    }

    /// Drops history beyond the retention window. Without this the table grows
    /// without bound for the lifetime of the deployment.
    pub async fn prune_older_than_days(&self, days: i64) -> Result<u64> {
        let cutoff = Utc::now() - chrono::Duration::days(days.max(1));

        let result = sqlx::query("DELETE FROM alert_events WHERE triggered_at < ?")
            .bind(cutoff)
            .execute(self.db.pool())
            .await?;

        Ok(result.rows_affected())
    }
}
