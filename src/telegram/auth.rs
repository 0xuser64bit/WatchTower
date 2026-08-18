use crate::db::repos::users::{Role, User, UserRepo};
use crate::db::Db;
use crate::error::{AppError, Result};
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
        .ok_or_else(|| AppError::Unauthorized)?;

    let user = UserRepo::new(db)
        .find_by_telegram_id(telegram_id)
        .await?
        .ok_or(AppError::Unauthorized)?;

    if user.is_blocked() {
        return Err(AppError::Forbidden);
    }

    Ok(AuthContext { user })
}

pub fn require_admin(ctx: &AuthContext) -> Result<()> {
    if ctx.user.role() != Role::Admin {
        return Err(AppError::Forbidden);
    }

    Ok(())
}

pub async fn authorize_or_send(bot: &Bot, db: &Db, msg: &Message) -> Option<AuthContext> {
    match authorize(db, msg).await {
        Ok(ctx) => Some(ctx),
        Err(AppError::Unauthorized) => {
            send_unauthorized(bot, msg.chat.id).await;
            None
        }
        Err(AppError::Forbidden) => {
            send_forbidden(bot, msg.chat.id).await;
            None
        }
        Err(err) => {
            tracing::warn!(%err, "authorization failed");
            let _ = bot
                .send_message(msg.chat.id, "Authorization failed due to an internal error.")
                .await;
            None
        }
    }
}

pub async fn authorize_admin_or_send(bot: &Bot, db: &Db, msg: &Message) -> Option<AuthContext> {
    let ctx = authorize_or_send(bot, db, msg).await?;

    if require_admin(&ctx).is_err() {
        send_forbidden(bot, msg.chat.id).await;
        return None;
    }

    Some(ctx)
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
