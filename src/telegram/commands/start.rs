use crate::db::Db;
use crate::telegram::auth;
use std::sync::Arc;
use teloxide::prelude::*;

pub async fn start(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ctx = match auth::authorize(&db, &msg).await {
        Ok(ctx) => ctx,
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    };

    let _ = bot
        .send_message(
            msg.chat.id,
            format!(
                "Welcome to ChainSentinel, Telegram ID {}.\n\nUse /help to see available commands.",
                ctx.user.telegram_id
            ),
        )
        .await?;

    Ok(())
}

pub async fn help(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ctx = match auth::authorize(&db, &msg).await {
        Ok(ctx) => ctx,
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    };

    let text = "ChainSentinel commands:\n/start - main menu\n/help - this help\n/addtoken - add a token to track\n/tokens - list tracked tokens";

    let _ = bot.send_message(msg.chat.id, text).await?;
    Ok(())
}
