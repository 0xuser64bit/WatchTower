pub mod auth;
pub mod commands;

use crate::db::Db;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;

type BotResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

pub async fn run(bot: Bot, db: Arc<Db>) {
    let command_handler = teloxide::filter_command::<commands::Command, _>()
        .endpoint(commands::dispatch);

    let handler = Update::filter_message().branch(command_handler).endpoint(commands::fallback);

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db, InMemStorage::<()>::new()])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
