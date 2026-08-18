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
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Role {
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "admin" => Ok(Role::Admin),
            "user" => Ok(Role::User),
            other => Err(AppError::Parse(format!("unknown role: {other}"))),
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub telegram_id: i64,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub blocked_at: Option<DateTime<Utc>>,
}

impl User {
    pub fn role(&self) -> Role {
        Role::parse(&self.role).unwrap_or(Role::User)
    }

    pub fn is_blocked(&self) -> bool {
        self.blocked_at.is_some()
    }
}

pub struct UserRepo<'a> {
    db: &'a Db,
}

impl<'a> UserRepo<'a> {
    pub fn new(db: &'a Db) -> Self {
        Self { db }
    }

    pub async fn find_by_telegram_id(&self, telegram_id: i64) -> Result<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT id, telegram_id, role, created_at, updated_at, blocked_at \
             FROM users WHERE telegram_id = ?",
        )
        .bind(telegram_id)
        .fetch_optional(self.db.pool())
        .await?;

        Ok(user)
    }

    pub async fn create(&self, telegram_id: i64, role: Role) -> Result<User> {
        sqlx::query("INSERT INTO users (telegram_id, role) VALUES (?, ?)")
            .bind(telegram_id)
            .bind(role.as_str())
            .execute(self.db.pool())
            .await?;

        self.find_by_telegram_id(telegram_id)
            .await?
            .ok_or_else(|| AppError::NotFound("created user not found".into()))
    }

    pub async fn list(&self) -> Result<Vec<User>> {
        let users = sqlx::query_as::<_, User>(
            "SELECT id, telegram_id, role, created_at, updated_at, blocked_at \
             FROM users ORDER BY created_at ASC",
        )
        .fetch_all(self.db.pool())
        .await?;

        Ok(users)
    }

    pub async fn list_admins(&self) -> Result<Vec<User>> {
        let users = sqlx::query_as::<_, User>(
            "SELECT id, telegram_id, role, created_at, updated_at, blocked_at \
             FROM users WHERE role = 'admin' AND blocked_at IS NULL ORDER BY created_at ASC",
        )
        .fetch_all(self.db.pool())
        .await?;

        Ok(users)
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
            return Err(AppError::NotFound(format!(
                "user with telegram id {telegram_id} not found"
            )));
        }

        Ok(())
    }

    pub async fn set_blocked(&self, telegram_id: i64, blocked: bool) -> Result<()> {
        let blocked_at = if blocked { Some(Utc::now()) } else { None };

        let result = sqlx::query(
            "UPDATE users SET blocked_at = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
             WHERE telegram_id = ?",
        )
        .bind(blocked_at)
        .bind(telegram_id)
        .execute(self.db.pool())
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "user with telegram id {telegram_id} not found"
            )));
        }

        Ok(())
    }
}
