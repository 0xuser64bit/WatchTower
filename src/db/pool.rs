use crate::error::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

/// SQLite handle.
///
/// WAL plus `synchronous = NORMAL` is the standard durable-but-fast configuration:
/// committed transactions survive a process crash, and only an OS-level crash can
/// lose the most recent commits — an acceptable trade for alert bookkeeping, and far
/// cheaper than the `FULL` default on every write.
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    pub async fn connect(database_url: &str) -> Result<Self> {
        create_database_parent(database_url)?;

        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            // A single writer plus the control plane can contend briefly; waiting is
            // correct, failing the command is not.
            .busy_timeout(Duration::from_secs(15));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(20))
            .connect_with(options)
            .await?;

        Ok(Self { pool })
    }

    /// In-memory database for tests. `min_connections(1)` and no idle timeout keep the
    /// single connection alive, since closing it would discard the whole database.
    pub async fn connect_in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await?;

        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    /// Cheap liveness probe used by `/status` and at startup.
    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// Flushes the WAL into the main database file so a copy of it is self-contained.
    pub async fn checkpoint(&self) -> Result<()> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

fn create_database_parent(database_url: &str) -> Result<()> {
    let Some(path) = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
    else {
        return Ok(());
    };

    // Query strings such as `?mode=rwc` are not part of the filesystem path.
    let path = path.split('?').next().unwrap_or(path);

    if path.is_empty() || path == ":memory:" {
        return Ok(());
    }

    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_non_file_urls_and_query_strings() {
        assert!(create_database_parent("sqlite::memory:").is_ok());
        assert!(create_database_parent("postgres://x").is_ok());
        assert!(create_database_parent("sqlite://:memory:").is_ok());
    }

    #[tokio::test]
    async fn in_memory_database_survives_pool_reuse() {
        let db = Db::connect_in_memory().await.unwrap();
        db.migrate().await.unwrap();

        // Two sequential acquisitions must see the same database.
        sqlx::query("INSERT INTO users (telegram_id, role) VALUES (1, 'admin')")
            .execute(db.pool())
            .await
            .unwrap();

        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(db.pool())
            .await
            .unwrap();

        assert_eq!(count, 1);
        db.ping().await.unwrap();
    }
}
