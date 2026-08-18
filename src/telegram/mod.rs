pub mod auth;
pub mod commands;
pub mod flows;

use crate::db::Db;
use std::sync::Arc;
use teloxide::dispatching::{dialogue, dialogue::InMemStorage, UpdateHandler};
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;
use tokio_util::sync::CancellationToken;

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
    #[command(description = "add a wallet to track")]
    Addwallet,
    #[command(description = "list tracked wallets")]
    Wallets,
    #[command(description = "add an alert rule")]
    Addalert,
    #[command(description = "list alert rules")]
    Alerts,
    #[command(description = "show recent alerts")]
    History,
    #[command(description = "open the admin panel")]
    Admin,
    #[command(description = "list authorized users")]
    Listusers,
    #[command(description = "grant admin")]
    Addadmin(String),
    #[command(description = "revoke admin")]
    Demote(String),
    #[command(description = "block user")]
    Block(String),
    #[command(description = "unblock user")]
    Unblock(String),
}

pub fn schema() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    let command_handler = teloxide::filter_command::<Command, _>()
        .branch(case![Command::Start].endpoint(commands::start::start))
        .branch(case![Command::Help].endpoint(commands::start::help))
        .branch(case![Command::Addtoken].endpoint(commands::tokens::start_add_token))
        .branch(case![Command::Tokens].endpoint(commands::tokens::list_tokens))
        .branch(case![Command::Addwallet].endpoint(commands::wallets::start_add_wallet))
        .branch(case![Command::Wallets].endpoint(commands::wallets::list_wallets))
        .branch(case![Command::Addalert].endpoint(commands::start_add_alert))
        .branch(case![Command::Alerts].endpoint(commands::alerts::list_alerts))
        .branch(case![Command::History].endpoint(commands::alerts::show_history))
        .branch(case![Command::Admin].endpoint(commands::admin::admin_menu))
        .branch(case![Command::Listusers].endpoint(commands::admin::list_users))
        .branch(case![Command::Addadmin(telegram_id)].endpoint(
            |bot: Bot, db: Arc<Db>, msg: Message, telegram_id: String| async move {
                commands::admin::add_admin(bot, db, msg, telegram_id).await
            },
        ))
        .branch(case![Command::Demote(telegram_id)].endpoint(
            |bot: Bot, db: Arc<Db>, msg: Message, telegram_id: String| async move {
                commands::admin::demote_user(bot, db, msg, telegram_id).await
            },
        ))
        .branch(case![Command::Block(telegram_id)].endpoint(
            |bot: Bot, db: Arc<Db>, msg: Message, telegram_id: String| async move {
                commands::admin::block_user(bot, db, msg, telegram_id).await
            },
        ))
        .branch(case![Command::Unblock(telegram_id)].endpoint(
            |bot: Bot, db: Arc<Db>, msg: Message, telegram_id: String| async move {
                commands::admin::unblock_user(bot, db, msg, telegram_id).await
            },
        ));

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
    .branch(
        dialogue::enter::<
            Update,
            InMemStorage<flows::add_wallet::AddWalletState>,
            flows::add_wallet::AddWalletState,
            _,
        >()
        .branch(flows::add_wallet::message_handler()),
    )
    .branch(message_handler)
}

pub async fn run(bot: Bot, db: Arc<Db>, shutdown: CancellationToken) {
    let mut dispatcher = Dispatcher::builder(bot, schema())
        .dependencies(dptree::deps![
            db,
            InMemStorage::<flows::add_token::AddTokenState>::new(),
            InMemStorage::<flows::add_alert::AddAlertState>::new(),
            InMemStorage::<flows::add_wallet::AddWalletState>::new()
        ])
        .build();

    tokio::select! {
        _ = shutdown.cancelled() => {
            tracing::info!("telegram dispatcher received shutdown signal");
        }
        _ = dispatcher.dispatch() => {}
    }
}
