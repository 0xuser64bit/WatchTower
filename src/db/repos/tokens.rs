use crate::db::repos::rules::is_unique_violation;
use crate::db::Db;
use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Token {
    pub id: i64,
    pub mint_address: String,
    pub symbol: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Token {
    pub fn display(&self) -> String {
        match &self.symbol {
            Some(symbol) => format!("{symbol} ({})", self.mint_address),
            None => self.mint_address.clone(),
        }
    }
}

/// A token together with how many alert rules depend on it, so the UI can warn
/// before a cascading delete.
#[derive(Debug, Clone, FromRow)]
pub struct TokenWithRules {
    pub id: i64,
    pub mint_address: String,
    pub symbol: Option<String>,
    pub rule_count: i64,
}

pub struct TokenRepo<'a> {
    db: &'a Db,
}

impl<'a> TokenRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn find_by_mint(&self, mint: &str) -> Result<Option<Token>> {
        Ok(sqlx::query_as::<_, Token>(
            "SELECT id, mint_address, symbol, created_at FROM tokens WHERE mint_address = ?",
        )
        .bind(mint)
        .fetch_optional(self.db.pool())
        .await?)
    }

    pub async fn find(&self, id: i64) -> Result<Option<Token>> {
        Ok(sqlx::query_as::<_, Token>(
            "SELECT id, mint_address, symbol, created_at FROM tokens WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?)
    }

    pub async fn create(&self, mint_address: &str, symbol: Option<&str>) -> Result<Token> {
        let result = sqlx::query("INSERT INTO tokens (mint_address, symbol) VALUES (?, ?)")
            .bind(mint_address)
            .bind(symbol)
            .execute(self.db.pool())
            .await;

        match result {
            Ok(result) => self
                .find(result.last_insert_rowid())
                .await?
                .ok_or_else(|| AppError::Data("token vanished immediately after insert".into())),
            Err(err) if is_unique_violation(&err) => Err(AppError::Conflict(
                "this token is already tracked".to_string(),
            )),
            Err(err) => Err(err.into()),
        }
    }

    pub async fn list(&self) -> Result<Vec<TokenWithRules>> {
        Ok(sqlx::query_as::<_, TokenWithRules>(
            "SELECT t.id, t.mint_address, t.symbol, \
                    (SELECT COUNT(*) FROM rules r WHERE r.token_id = t.id) AS rule_count \
             FROM tokens t ORDER BY t.id",
        )
        .fetch_all(self.db.pool())
        .await?)
    }

    /// Deletes the token and, by `ON DELETE CASCADE`, every rule watching it.
    /// Returns the number of rules removed so the user is told what else went.
    pub async fn delete(&self, id: i64) -> Result<i64> {
        let mut tx = self.db.pool().begin().await?;

        let (rule_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM rules WHERE token_id = ?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;

        let result = sqlx::query("DELETE FROM tokens WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(AppError::NotFound(format!("token {id}")));
        }

        tx.commit().await?;
        Ok(rule_count)
    }

    pub async fn count(&self) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tokens")
            .fetch_one(self.db.pool())
            .await?;
        Ok(count)
    }
}
