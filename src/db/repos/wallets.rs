use crate::db::Db;
use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Wallet {
    pub id: i64,
    pub address: String,
    pub label: Option<String>,
    pub last_seen_signature: Option<String>,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

pub struct WalletRepo<'a> {
    db: &'a Db,
}

impl<'a> WalletRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn find_by_address(&self, address: &str) -> Result<Option<Wallet>> {
        let wallet = sqlx::query_as::<_, Wallet>(
            "SELECT id, address, label, last_seen_signature, created_at, deleted_at \
             FROM wallets WHERE address = ? AND deleted_at IS NULL",
        )
        .bind(address)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(wallet)
    }

    pub async fn create(&self, address: &str, label: Option<&str>) -> Result<Wallet> {
        sqlx::query("INSERT INTO wallets (address, label) VALUES (?, ?)")
            .bind(address)
            .bind(label)
            .execute(self.db.pool())
            .await?;

        self.find_by_address(address)
            .await?
            .ok_or_else(|| AppError::NotFound("created wallet not found".into()))
    }

    pub async fn list(&self) -> Result<Vec<Wallet>> {
        let wallets = sqlx::query_as::<_, Wallet>(
            "SELECT id, address, label, last_seen_signature, created_at, deleted_at \
             FROM wallets WHERE deleted_at IS NULL ORDER BY id DESC",
        )
        .fetch_all(self.db.pool())
        .await?;

        Ok(wallets)
    }

    pub async fn update_last_seen(&self, id: i64, signature: &str) -> Result<()> {
        let result = sqlx::query(
            "UPDATE wallets SET last_seen_signature = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(signature)
        .bind(id)
        .execute(self.db.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("wallet {id} not found")));
        }

        Ok(())
    }
}
