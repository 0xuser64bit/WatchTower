pub mod start;

use crate::db::Db;
use crate::error::AppError;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
pub enum Command {
    #[command(description = "show the main menu")]
    Start,
    #[command(description = "show help")]
    Help,
}

pub async fn dispatch(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    cmd: Command,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match cmd {
        Command::Start => start::start(bot, db, msg).await,
        Command::Help => start::help(bot, db, msg).await,
    }
}

pub async fn fallback(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = bot
        .send_message(msg.chat.id, "Use /start or /help to see the main menu.")
        .await?;
    Ok(())
}
