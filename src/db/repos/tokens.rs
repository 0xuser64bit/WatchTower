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
    /// When this token was starred, if it is. See [`TokenRepo::set_favourite`].
    pub favourited_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl Token {
    pub fn display(&self) -> String {
        match &self.symbol {
            Some(symbol) => format!("{symbol} ({})", self.mint_address),
            None => self.mint_address.clone(),
        }
    }

    pub fn is_favourite(&self) -> bool {
        self.favourited_at.is_some()
    }
}

/// A token together with how many alert rules depend on it, so the UI can warn
/// before a cascading delete.
#[derive(Debug, Clone, FromRow)]
pub struct TokenWithRules {
    pub id: i64,
    pub mint_address: String,
    pub symbol: Option<String>,
    pub favourited_at: Option<DateTime<Utc>>,
    pub rule_count: i64,
}

impl TokenWithRules {
    pub fn is_favourite(&self) -> bool {
        self.favourited_at.is_some()
    }
}

/// Columns of `tokens`, named explicitly so adding a column cannot silently change
/// what a `FromRow` sees.
const SELECT: &str = "SELECT id, mint_address, symbol, favourited_at, created_at FROM tokens";

/// Favourites first, oldest star first, then everything else by insertion order.
///
/// Applied to every listing rather than only the favourites screen: the point of
/// starring a token is that it stops being something to scroll past.
const ORDER: &str = "ORDER BY favourited_at IS NULL, favourited_at ASC, id ASC";

pub struct TokenRepo<'a> {
    db: &'a Db,
}

impl<'a> TokenRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn find_by_mint(&self, mint: &str) -> Result<Option<Token>> {
        Ok(
            sqlx::query_as::<_, Token>(&format!("{SELECT} WHERE mint_address = ?"))
                .bind(mint)
                .fetch_optional(self.db.pool())
                .await?,
        )
    }

    pub async fn find(&self, id: i64) -> Result<Option<Token>> {
        Ok(
            sqlx::query_as::<_, Token>(&format!("{SELECT} WHERE id = ?"))
                .bind(id)
                .fetch_optional(self.db.pool())
                .await?,
        )
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
        self.collect(&format!(
            "SELECT t.id, t.mint_address, t.symbol, t.favourited_at, \
                    (SELECT COUNT(*) FROM rules r WHERE r.token_id = t.id) AS rule_count \
             FROM tokens t {ORDER}"
        ))
        .await
    }

    /// The starred tokens, in the same order they appear at the top of the full list.
    pub async fn list_favourites(&self) -> Result<Vec<TokenWithRules>> {
        self.collect(&format!(
            "SELECT t.id, t.mint_address, t.symbol, t.favourited_at, \
                    (SELECT COUNT(*) FROM rules r WHERE r.token_id = t.id) AS rule_count \
             FROM tokens t WHERE t.favourited_at IS NOT NULL {ORDER}"
        ))
        .await
    }

    async fn collect(&self, sql: &str) -> Result<Vec<TokenWithRules>> {
        Ok(sqlx::query_as::<_, TokenWithRules>(sql)
            .fetch_all(self.db.pool())
            .await?)
    }

    /// Stars or unstars a token.
    ///
    /// Idempotent, and starring an already-starred token keeps its original timestamp:
    /// a double tap must not silently reorder the favourites list. Returns the stored
    /// token so the caller re-renders from the database rather than from what it hoped
    /// the write did.
    pub async fn set_favourite(&self, id: i64, favourite: bool) -> Result<Token> {
        let result = sqlx::query(
            "UPDATE tokens SET favourited_at = CASE \
                 WHEN ?1 THEN COALESCE(favourited_at, strftime('%Y-%m-%dT%H:%M:%fZ','now')) \
                 ELSE NULL END \
             WHERE id = ?2",
        )
        .bind(favourite)
        .bind(id)
        .execute(self.db.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("token {id}")));
        }

        self.find(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("token {id}")))
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

    pub async fn count_favourites(&self) -> Result<i64> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM tokens WHERE favourited_at IS NOT NULL")
                .fetch_one(self.db.pool())
                .await?;
        Ok(count)
    }
}
