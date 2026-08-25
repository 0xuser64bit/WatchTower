//! The Telegram control plane.

pub mod auth;
pub mod callback;
pub mod commands;
pub mod copy;
pub mod flows;
pub mod menu;
pub mod reply;
pub mod screens;
pub mod ui;

use crate::app_state::AppState;
use flows::{DialogueState, FlowDialogue};
use teloxide::dispatching::{dialogue, dialogue::InMemStorage, UpdateHandler};
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;

type HandlerError = Box<dyn std::error::Error + Send + Sync>;

/// Every argument is taken as `String` and parsed by the handler so invalid ids get a
/// command-specific usage reply.
#[derive(BotCommands, Clone, Debug, PartialEq)]
#[command(rename_rule = "lowercase", description = "WatchTower commands")]
pub enum Command {
    #[command(description = "open WatchTower")]
    Start,
    #[command(description = "open the menu")]
    Menu,
    #[command(description = "how it works")]
    Help,
    #[command(description = "engine and provider health")]
    Status,
    #[command(description = "abandon the current step")]
    Cancel,

    #[command(description = "track a token")]
    Addtoken,
    #[command(description = "list tracked tokens")]
    Tokens,
    #[command(description = "stop tracking a token")]
    Deletetoken(String),

    #[command(description = "track a wallet")]
    Addwallet,
    #[command(description = "list tracked wallets")]
    Wallets,
    #[command(description = "stop tracking a wallet")]
    Deletewallet(String),

    #[command(description = "create an alert rule")]
    Addalert,
    #[command(description = "list alert rules")]
    Alerts,
    #[command(description = "enable an alert rule")]
    Enablerule(String),
    #[command(description = "disable an alert rule")]
    Disablerule(String),
    #[command(description = "delete an alert rule")]
    Deleterule(String),
    #[command(description = "recent alerts")]
    History,

    #[command(description = "admin panel")]
    Admin,
    #[command(description = "list users")]
    Listusers,
    #[command(description = "grant admin")]
    Addadmin(String),
    #[command(description = "revoke admin")]
    Demote(String),
    #[command(description = "block a user")]
    Block(String),
    #[command(description = "unblock a user")]
    Unblock(String),
}

pub fn schema() -> UpdateHandler<HandlerError> {
    use dptree::case;

    let command_handler = teloxide::filter_command::<Command, _>()
        // A command always wins over an in-progress flow, and clears it first, so a
        // user can never be trapped in a dialogue and a stray flow step can never
        // consume the argument of an unrelated command.
        .chain(dptree::filter_map_async(
            |dialogue: FlowDialogue, current: DialogueState| async move {
                if current != DialogueState::Idle {
                    flows::reset(&dialogue).await;
                }
                Some(())
            },
        ))
        .branch(case![Command::Start].endpoint(commands::start))
        .branch(case![Command::Menu].endpoint(commands::menu))
        .branch(case![Command::Help].endpoint(commands::help))
        .branch(case![Command::Status].endpoint(commands::status::status))
        .branch(case![Command::Cancel].endpoint(flows::cancel))
        .branch(case![Command::Addtoken].endpoint(flows::add_token::start))
        .branch(case![Command::Tokens].endpoint(commands::targets::list_tokens))
        .branch(case![Command::Deletetoken(args)].endpoint(commands::targets::delete_token))
        .branch(case![Command::Addwallet].endpoint(flows::add_wallet::start))
        .branch(case![Command::Wallets].endpoint(commands::targets::list_wallets))
        .branch(case![Command::Deletewallet(args)].endpoint(commands::targets::delete_wallet))
        .branch(case![Command::Addalert].endpoint(flows::add_alert::start))
        .branch(case![Command::Alerts].endpoint(commands::alerts::list_rules))
        .branch(case![Command::Enablerule(args)].endpoint(
            |state: AppState, msg: Message, args: String| async move {
                commands::alerts::set_enabled(state, msg, args, true).await
            },
        ))
        .branch(case![Command::Disablerule(args)].endpoint(
            |state: AppState, msg: Message, args: String| async move {
                commands::alerts::set_enabled(state, msg, args, false).await
            },
        ))
        .branch(case![Command::Deleterule(args)].endpoint(commands::alerts::delete_rule))
        .branch(case![Command::History].endpoint(commands::alerts::history))
        .branch(case![Command::Admin].endpoint(commands::admin::panel))
        .branch(case![Command::Listusers].endpoint(commands::admin::list_users))
        .branch(case![Command::Addadmin(args)].endpoint(commands::admin::add_admin))
        .branch(case![Command::Demote(args)].endpoint(commands::admin::demote))
        .branch(case![Command::Block(args)].endpoint(
            |state: AppState, msg: Message, args: String| async move {
                commands::admin::set_blocked(state, msg, args, true).await
            },
        ))
        .branch(case![Command::Unblock(args)].endpoint(
            |state: AppState, msg: Message, args: String| async move {
                commands::admin::set_blocked(state, msg, args, false).await
            },
        ));

    // Order is load-bearing.
    //
    // 1. Non-private chats are refused outright. Dialogue state is keyed by chat, so
    //    in a group an admin starting a flow would make the next message from *any*
    //    member the answer to that step — an unauthorized user could supply the mint
    //    or send the confirmation. Alerts are also delivered to individual admins,
    //    so group operation was never coherent.
    // 2. Commands, which clear any flow first.
    // 3. Steps of an active flow, re-authorized on every message so that blocking a
    //    user takes effect immediately even mid-flow.
    // 4. Everything else.
    let message_handler = Update::filter_message()
        .branch(
            dptree::filter(|msg: Message| !msg.chat.is_private())
                .endpoint(commands::non_private_chat),
        )
        .branch(command_handler)
        .branch(
            dptree::filter_map_async(|state: AppState, msg: Message| async move {
                // Returns `None` on denial so the request falls through to the
                // fallback branch, which issues exactly one denial reply.
                reply::is_authorized(&state.db, &msg).await.then_some(())
            })
            .chain(flows::handler()),
        )
        .branch(dptree::endpoint(commands::fallback));

    dialogue::enter::<Update, InMemStorage<DialogueState>, DialogueState, _>()
        .branch(message_handler)
        // Callback queries share the same dialogue (keyed by chat), so a tapped button
        // can advance the very flow a typed message started. Authorization and answering
        // the query happen inside the handler.
        .branch(callback::handler())
}

pub async fn run(state: AppState) {
    // Publishes the `/` autocomplete list. Without it the bot has commands but no
    // discoverable UI, and a new user has to be told they exist.
    menu::publish(&state.bot, &state.db).await;

    let state_shutdown = state.shutdown.clone();

    let mut dispatcher = Dispatcher::builder(state.bot.clone(), schema())
        .dependencies(dptree::deps![state, InMemStorage::<DialogueState>::new()])
        .default_handler(|update| async move {
            // Non-message updates (edits, callbacks, channel posts). Expected, but
            // recorded at debug so an unexpected flood is diagnosable.
            tracing::debug!(update_id = update.id.0, "unhandled update kind");
        })
        // Endpoint errors are reported to the user by the handlers themselves; this
        // is the last resort for anything that escaped, and it must never be silent.
        .error_handler(std::sync::Arc::new(|err| async move {
            tracing::error!(error = %err, "unhandled error in telegram handler");
        }))
        .build();

    let dispatcher_shutdown = dispatcher.shutdown_token();
    let shutdown = state_shutdown;

    // Ask the dispatcher to stop accepting updates and drain in-flight handlers.
    let waiter = tokio::spawn(async move {
        shutdown.cancelled().await;
        tracing::info!("telegram dispatcher shutting down");
        if let Err(err) = dispatcher_shutdown.shutdown() {
            tracing::debug!(%err, "dispatcher was not running");
        }
    });

    dispatcher.dispatch().await;
    waiter.abort();

    tracing::info!("telegram dispatcher stopped");
}
