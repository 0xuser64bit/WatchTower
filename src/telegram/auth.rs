use crate::db::repos::users::{Role, User, UserRepo};
use crate::db::Db;
use crate::error::Result;
use teloxide::prelude::*;
use teloxide::types::ChatId;

#[derive(Clone)]
pub struct AuthContext {
    pub user: User,
}

pub async fn authorize(db: &Db, message: &Message) -> Result<AuthContext> {
    let telegram_id = message
        .from()
        .map(|user| user.id.0 as i64)
        .ok_or_else(|| crate::error::AppError::Unauthorized)?;

    let user = UserRepo::new(db)
        .find_by_telegram_id(telegram_id)
        .await?
        .ok_or_else(|| crate::error::AppError::Unauthorized)?;

    if user.is_blocked() {
        return Err(crate::error::AppError::Forbidden);
    }

    Ok(AuthContext { user })
}

pub fn require_admin(ctx: &AuthContext) -> Result<()> {
    if ctx.user.role() != Role::Admin {
        return Err(crate::error::AppError::Forbidden);
    }

    Ok(())
}

pub async fn send_unauthorized(bot: &Bot, chat_id: ChatId) {
    let _ = bot
        .send_message(chat_id, "You are not authorized to use this bot.")
        .await;
}

pub async fn send_forbidden(bot: &Bot, chat_id: ChatId) {
    let _ = bot
        .send_message(chat_id, "This action requires admin privileges.")
        .await;
}
