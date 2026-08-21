use crate::db::Db;
use crate::error::{AppError, Result};
use chrono::{DateTime, Utc};
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }

    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "admin" => Ok(Role::Admin),
            "user" => Ok(Role::User),
            other => Err(AppError::Data(format!("unknown user role `{other}`"))),
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The minimal authenticated identity carried through request handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthUser {
    pub telegram_id: i64,
    pub role: Role,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    pub telegram_id: i64,
    pub role: Role,
    pub blocked: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(FromRow)]
struct UserRow {
    id: i64,
    telegram_id: i64,
    role: String,
    created_at: DateTime<Utc>,
    blocked_at: Option<DateTime<Utc>>,
}

impl TryFrom<UserRow> for User {
    type Error = AppError;

    fn try_from(row: UserRow) -> Result<Self> {
        Ok(User {
            id: row.id,
            telegram_id: row.telegram_id,
            role: Role::parse(&row.role)?,
            blocked: row.blocked_at.is_some(),
            created_at: row.created_at,
        })
    }
}

const SELECT: &str = "SELECT id, telegram_id, role, created_at, blocked_at FROM users";

pub struct UserRepo<'a> {
    db: &'a Db,
}

impl<'a> UserRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn find_by_telegram_id(&self, telegram_id: i64) -> Result<Option<User>> {
        sqlx::query_as::<_, UserRow>(&format!("{SELECT} WHERE telegram_id = ?"))
            .bind(telegram_id)
            .fetch_optional(self.db.pool())
            .await?
            .map(User::try_from)
            .transpose()
    }

    /// Inserts the user if absent, otherwise updates the role. Idempotent so admin
    /// seeding and `/addadmin` cannot race into a unique-constraint failure.
    pub async fn upsert(&self, telegram_id: i64, role: Role) -> Result<User> {
        sqlx::query(
            "INSERT INTO users (telegram_id, role) VALUES (?, ?) \
             ON CONFLICT(telegram_id) DO UPDATE SET \
                 role = excluded.role, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')",
        )
        .bind(telegram_id)
        .bind(role.as_str())
        .execute(self.db.pool())
        .await?;

        self.find_by_telegram_id(telegram_id)
            .await?
            .ok_or_else(|| AppError::Data("user vanished immediately after upsert".into()))
    }

    pub async fn list(&self) -> Result<Vec<User>> {
        sqlx::query_as::<_, UserRow>(&format!("{SELECT} ORDER BY created_at ASC, id ASC"))
            .fetch_all(self.db.pool())
            .await?
            .into_iter()
            .map(User::try_from)
            .collect()
    }

    /// Active admins, i.e. the set that receives alerts.
    pub async fn list_active_admins(&self) -> Result<Vec<User>> {
        sqlx::query_as::<_, UserRow>(&format!(
            "{SELECT} WHERE role = 'admin' AND blocked_at IS NULL ORDER BY created_at ASC, id ASC"
        ))
        .fetch_all(self.db.pool())
        .await?
        .into_iter()
        .map(User::try_from)
        .collect()
    }

    pub async fn count_active_admins(&self) -> Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM users WHERE role = 'admin' AND blocked_at IS NULL",
        )
        .fetch_one(self.db.pool())
        .await?;
        Ok(count)
    }

    pub async fn set_role(&self, telegram_id: i64, role: Role) -> Result<()> {
        let result = sqlx::query(
            "UPDATE users SET role = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE telegram_id = ?",
        )
        .bind(role.as_str())
        .bind(telegram_id)
        .execute(self.db.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("user {telegram_id}")));
        }

        Ok(())
    }

    pub async fn set_blocked(&self, telegram_id: i64, blocked: bool) -> Result<()> {
        let result = sqlx::query(
            "UPDATE users \
             SET blocked_at = CASE WHEN ?1 THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') ELSE NULL END, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE telegram_id = ?2",
        )
        .bind(blocked)
        .bind(telegram_id)
        .execute(self.db.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("user {telegram_id}")));
        }

        Ok(())
    }
}
