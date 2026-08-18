use crate::db::Db;
use crate::error::Result;
use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct AlertEvent {
    pub id: i64,
    pub rule_id: i64,
    pub current_value: f64,
    pub threshold_value: f64,
    pub message: String,
    pub dedup_key: String,
    pub triggered_at: DateTime<Utc>,
}

pub struct AlertEventRepo<'a> {
    db: &'a Db,
}

impl<'a> AlertEventRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn insert(
        &self,
        rule_id: i64,
        current_value: f64,
        threshold_value: f64,
        message: &str,
        dedup_key: &str,
    ) -> Result<AlertEvent> {
        sqlx::query(
            "INSERT INTO alert_events (rule_id, current_value, threshold_value, message, dedup_key) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(rule_id)
        .bind(current_value)
        .bind(threshold_value)
        .bind(message)
        .bind(dedup_key)
        .execute(self.db.pool())
        .await?;

        self.find_by_dedup_key(dedup_key)
            .await?
            .ok_or_else(|| crate::error::AppError::NotFound("created alert event not found".into()))
    }

    pub async fn find_by_dedup_key(&self, dedup_key: &str) -> Result<Option<AlertEvent>> {
        let event = sqlx::query_as::<_, AlertEvent>(
            "SELECT id, rule_id, current_value, threshold_value, message, dedup_key, triggered_at \
             FROM alert_events WHERE dedup_key = ?",
        )
        .bind(dedup_key)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(event)
    }

    pub async fn list_recent(&self, limit: i64) -> Result<Vec<AlertEvent>> {
        let events = sqlx::query_as::<_, AlertEvent>(
            "SELECT id, rule_id, current_value, threshold_value, message, dedup_key, triggered_at \
             FROM alert_events ORDER BY triggered_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.db.pool())
        .await?;

        Ok(events)
    }
}
