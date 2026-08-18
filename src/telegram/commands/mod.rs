pub mod start;
pub mod tokens;

use crate::db::Db;
use std::sync::Arc;
use teloxide::prelude::*;

pub async fn fallback(
    bot: Bot,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = bot
        .send_message(msg.chat.id, "Use /help to see available commands.")
        .await?;
    Ok(())
}
