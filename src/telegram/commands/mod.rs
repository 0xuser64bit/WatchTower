pub mod admin;
pub mod alerts;
pub mod start;
pub mod tokens;
pub mod wallets;

use crate::db::Db;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;

pub async fn start_add_alert(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    dialogue: Dialogue<
        crate::telegram::flows::add_alert::AddAlertState,
        InMemStorage<crate::telegram::flows::add_alert::AddAlertState>,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if crate::telegram::auth::authorize_or_send(&bot, &db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    bot.send_message(
        msg.chat.id,
        "What kind of alert? Send `price` or `balance`.",
    )
    .await?;

    dialogue
        .update(crate::telegram::flows::add_alert::AddAlertState::AwaitingKind)
        .await?;

    Ok(())
}

pub async fn fallback(
    bot: Bot,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = bot
        .send_message(msg.chat.id, "Use /help to see available commands.")
        .await?;
    Ok(())
}
