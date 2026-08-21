//! Command handlers.

pub mod admin;
pub mod alerts;
pub mod status;
pub mod targets;

use crate::app_state::AppState;
use crate::db::repos::users::Role;
use crate::telegram::flows::HandlerResult;
use crate::telegram::reply;
use teloxide::prelude::*;

const HELP: &str = "\
ChainSentinel watches Solana tokens and wallets and messages you when a rule matches.

Targets
/addtoken - track a token by mint address
/tokens - list tracked tokens
/deletetoken <id> - stop tracking a token (also deletes its alerts)
/addwallet - track a wallet by address
/wallets - list tracked wallets
/deletewallet <id> - stop tracking a wallet (also deletes its alerts)

Alerts
/addalert - create an alert rule
/alerts - list alert rules and their state
/enablerule <id> - enable a rule
/disablerule <id> - disable a rule
/deleterule <id> - delete a rule
/history - recent alerts

Other
/status - engine and provider health
/cancel - abandon the current step
/help - this message";

const ADMIN_HELP: &str = "

Admin
/admin - admin panel
/listusers - list users
/addadmin <telegram_id> - grant admin
/demote <telegram_id> - revoke admin
/block <telegram_id> - block a user
/unblock <telegram_id> - unblock a user";

pub async fn start(state: AppState, msg: Message) -> HandlerResult {
    let Some(user) = reply::require_user(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let mut text = format!(
        "Welcome to ChainSentinel.\n\nYou are signed in as Telegram id {} ({}).\n\n{HELP}",
        user.telegram_id, user.role
    );

    if user.role == Role::Admin {
        text.push_str(ADMIN_HELP);
    }

    reply::send_text(&state.bot, msg.chat.id, text).await?;
    Ok(())
}

pub async fn help(state: AppState, msg: Message) -> HandlerResult {
    let Some(user) = reply::require_user(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let mut text = HELP.to_string();
    if user.role == Role::Admin {
        text.push_str(ADMIN_HELP);
    }

    reply::send_text(&state.bot, msg.chat.id, text).await?;
    Ok(())
}

/// Refuses group, supergroup, and channel chats.
///
/// Replies only to an apparent command attempt: answering every message in a group
/// the bot happens to be in would be noise, and could trip Telegram's flood limits.
pub async fn non_private_chat(state: AppState, msg: Message) -> HandlerResult {
    let looks_like_a_command = msg.text().is_some_and(|text| text.starts_with('/'));

    if looks_like_a_command {
        tracing::info!(
            chat_id = msg.chat.id.0,
            "refused a command from a non-private chat"
        );
        reply::try_send(
            &state.bot,
            msg.chat.id,
            "ChainSentinel only works in a direct message. Open a private chat with me \
             and send /start.",
        )
        .await;
    }

    Ok(())
}

/// Anything that is not a command and not part of an active flow.
pub async fn fallback(state: AppState, msg: Message) -> HandlerResult {
    // Authorize first: an unknown sender must not learn anything about the bot.
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    reply::send_text(
        &state.bot,
        msg.chat.id,
        "I only understand commands. Send /help to see them.",
    )
    .await?;
    Ok(())
}

/// Parses a positive row id from a command argument, replying with usage on failure.
pub async fn parse_id(state: &AppState, msg: &Message, raw: &str, usage: &str) -> Option<i64> {
    match raw.trim().parse::<i64>() {
        Ok(id) if id > 0 => Some(id),
        // Previously `/enablerule abc` failed to match the typed command branch and
        // fell through to the generic fallback, so the user was told to read /help
        // instead of being told the id was invalid.
        _ => {
            reply::try_send(&state.bot, msg.chat.id, format!("Usage: {usage}")).await;
            None
        }
    }
}
