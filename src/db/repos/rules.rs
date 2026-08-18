use crate::db::Db;
use crate::error::{AppError, Result};
use crate::rules::types::Rule;
use chrono::{DateTime, Utc};

pub struct RuleRepo<'a> {
    db: &'a Db,
}

impl<'a> RuleRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<Rule>> {
        let rule = sqlx::query_as::<_, Rule>(
            "SELECT id, kind, target_type, target_ref, metric, operator, threshold, \
                    time_window_seconds, cooldown_seconds, max_triggers, reference_value, \
                    enabled, created_at, updated_at, deleted_at \
             FROM rules WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(rule)
    }

    pub async fn list_enabled(&self) -> Result<Vec<Rule>> {
        let rules = sqlx::query_as::<_, Rule>(
            "SELECT id, kind, target_type, target_ref, metric, operator, threshold, \
                    time_window_seconds, cooldown_seconds, max_triggers, reference_value, \
                    enabled, created_at, updated_at, deleted_at \
             FROM rules WHERE enabled = 1 AND deleted_at IS NULL ORDER BY id ASC",
        )
        .fetch_all(self.db.pool())
        .await?;

        Ok(rules)
    }

    pub async fn list_all(&self) -> Result<Vec<Rule>> {
        let rules = sqlx::query_as::<_, Rule>(
            "SELECT id, kind, target_type, target_ref, metric, operator, threshold, \
                    time_window_seconds, cooldown_seconds, max_triggers, reference_value, \
                    enabled, created_at, updated_at, deleted_at \
             FROM rules WHERE deleted_at IS NULL ORDER BY id DESC",
        )
        .fetch_all(self.db.pool())
        .await?;

        Ok(rules)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        kind: &str,
        target_type: &str,
        target_ref: &str,
        metric: &str,
        operator: &str,
        threshold: f64,
        time_window_seconds: Option<i64>,
        cooldown_seconds: i64,
        max_triggers: Option<i64>,
        reference_value: Option<f64>,
    ) -> Result<Rule> {
        let result = sqlx::query(
            "INSERT INTO rules \
             (kind, target_type, target_ref, metric, operator, threshold, \
              time_window_seconds, cooldown_seconds, max_triggers, reference_value) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(kind)
        .bind(target_type)
        .bind(target_ref)
        .bind(metric)
        .bind(operator)
        .bind(threshold)
        .bind(time_window_seconds)
        .bind(cooldown_seconds)
        .bind(max_triggers)
        .bind(reference_value)
        .execute(self.db.pool())
        .await?;

        let id = result.last_insert_rowid();
        self.find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound("created rule not found".into()))
    }

    pub async fn set_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        let result = sqlx::query(
            "UPDATE rules SET enabled = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(if enabled { 1 } else { 0 })
        .bind(id)
        .execute(self.db.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("rule {id} not found")));
        }

        Ok(())
    }

    pub async fn initialize_reference_if_missing(&self, id: i64, value: f64) -> Result<()> {
        sqlx::query(
            "UPDATE rules \
             SET reference_value = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ? AND deleted_at IS NULL AND reference_value IS NULL",
        )
        .bind(value)
        .bind(id)
        .execute(self.db.pool())
        .await?;

        Ok(())
    }

    pub async fn soft_delete(&self, id: i64) -> Result<()> {
        let result = sqlx::query(
            "UPDATE rules SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(self.db.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("rule {id} not found")));
        }

        Ok(())
    }

    pub async fn count_triggers_since(&self, rule_id: i64, since: DateTime<Utc>) -> Result<i64> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alert_events WHERE rule_id = ? AND triggered_at >= ?",
        )
        .bind(rule_id)
        .bind(since)
        .fetch_one(self.db.pool())
        .await?;

        Ok(count.0)
    }

    pub async fn last_trigger_at(&self, rule_id: i64) -> Result<Option<DateTime<Utc>>> {
        let row: Option<(DateTime<Utc>,)> = sqlx::query_as(
            "SELECT triggered_at FROM alert_events WHERE rule_id = ? ORDER BY triggered_at DESC LIMIT 1",
        )
        .bind(rule_id)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(row.map(|r| r.0))
    }
}
