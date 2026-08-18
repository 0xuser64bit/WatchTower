pub mod auth;
pub mod commands;
pub mod flows;

use crate::db::Db;
use std::sync::Arc;
use teloxide::dispatching::{dialogue, dialogue::InMemStorage, UpdateHandler};
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "Available commands:")]
pub enum Command {
    #[command(description = "show the main menu")]
    Start,
    #[command(description = "show help")]
    Help,
    #[command(description = "add a token to track")]
    Addtoken,
    #[command(description = "list tracked tokens")]
    Tokens,
    #[command(description = "add an alert rule")]
    Addalert,
    #[command(description = "list alert rules")]
    Alerts,
    #[command(description = "show recent alerts")]
    History,
}

pub fn schema() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    let command_handler = teloxide::filter_command::<Command, _>()
        .branch(case![Command::Start].endpoint(commands::start::start))
        .branch(case![Command::Help].endpoint(commands::start::help))
        .branch(case![Command::Addtoken].endpoint(commands::tokens::start_add_token))
        .branch(case![Command::Tokens].endpoint(commands::tokens::list_tokens))
        .branch(case![Command::Addalert].endpoint(commands::start_add_alert))
        .branch(case![Command::Alerts].endpoint(commands::alerts::list_alerts))
        .branch(case![Command::History].endpoint(commands::alerts::show_history));

    let message_handler = Update::filter_message()
        .branch(command_handler)
        .branch(dptree::endpoint(commands::fallback));

    dialogue::enter::<
        Update,
        InMemStorage<flows::add_token::AddTokenState>,
        flows::add_token::AddTokenState,
        _,
    >()
    .branch(flows::add_token::message_handler())
    .branch(
        dialogue::enter::<
            Update,
            InMemStorage<flows::add_alert::AddAlertState>,
            flows::add_alert::AddAlertState,
            _,
        >()
        .branch(flows::add_alert::message_handler()),
    )
    .branch(message_handler)
}

pub async fn run(bot: Bot, db: Arc<Db>) {
    Dispatcher::builder(bot, schema())
        .dependencies(dptree::deps![
            db,
            InMemStorage::<flows::add_token::AddTokenState>::new(),
            InMemStorage::<flows::add_alert::AddAlertState>::new()
        ])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
