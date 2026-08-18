use crate::db::Db;
use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Token {
    pub id: i64,
    pub mint_address: String,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

pub struct TokenRepo<'a> {
    db: &'a Db,
}

impl<'a> TokenRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn find_by_mint(&self, mint: &str) -> Result<Option<Token>> {
        let token = sqlx::query_as::<_, Token>(
            "SELECT id, mint_address, symbol, name, created_at, deleted_at \
             FROM tokens WHERE mint_address = ? AND deleted_at IS NULL",
        )
        .bind(mint)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(token)
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<Token>> {
        let token = sqlx::query_as::<_, Token>(
            "SELECT id, mint_address, symbol, name, created_at, deleted_at \
             FROM tokens WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(token)
    }

    pub async fn create(
        &self,
        mint_address: &str,
        symbol: Option<&str>,
        name: Option<&str>,
    ) -> Result<Token> {
        sqlx::query("INSERT INTO tokens (mint_address, symbol, name) VALUES (?, ?, ?)")
            .bind(mint_address)
            .bind(symbol)
            .bind(name)
            .execute(self.db.pool())
            .await?;

        self.find_by_mint(mint_address)
            .await?
            .ok_or_else(|| AppError::NotFound("created token not found".into()))
    }

    pub async fn list(&self) -> Result<Vec<Token>> {
        let tokens = sqlx::query_as::<_, Token>(
            "SELECT id, mint_address, symbol, name, created_at, deleted_at \
             FROM tokens WHERE deleted_at IS NULL ORDER BY id DESC",
        )
        .fetch_all(self.db.pool())
        .await?;

        Ok(tokens)
    }

    pub async fn soft_delete(&self, id: i64) -> Result<()> {
        let result = sqlx::query(
            "UPDATE tokens SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(self.db.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("token {id} not found")));
        }

        Ok(())
    }
}
