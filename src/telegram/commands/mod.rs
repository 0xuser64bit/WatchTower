//! Command handlers.
//!
//! Slash commands are kept as shortcuts, but each now opens the same screen a button
//! would: `/alerts` shows the Alerts screen, `/menu` the main menu, and so on. The
//! primary way around is the inline keyboards those screens carry.

pub mod admin;
pub mod alerts;
pub mod status;
pub mod targets;

use crate::app_state::AppState;
use crate::db::repos::users::{AuthUser, Role, UserRepo};
use crate::error::Result;
use crate::telegram::flows::HandlerResult;
use crate::telegram::ui::Surface;
use crate::telegram::{copy, reply, screens};
use teloxide::prelude::*;

pub async fn start(state: AppState, msg: Message) -> HandlerResult {
    let Some(user) = reply::require_user(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let outcome = render_start(&state, &msg, user).await;
    reply::finish(&state.bot, msg.chat.id, "start", outcome).await
}

/// `/start` and `/menu` both open the main menu. The whole interface is one tap away,
/// so a new user is never handed a wall of commands.
async fn render_start(state: &AppState, msg: &Message, user: AuthUser) -> Result<()> {
    screens::show_main(state, Surface::New(msg.chat.id), user.role == Role::Admin).await?;

    // Surfaced only to admins, and only when it matters: an admin whose alerts have
    // nowhere to land needs to know now, not on their next /status.
    if user.role == Role::Admin && UserRepo::new(&state.db).count_active_admins().await? == 0 {
        reply::send_text(&state.bot, msg.chat.id, copy::NO_ADMINS_WARNING).await?;
    }

    Ok(())
}

/// `/menu` — the same entry point as `/start`, for people who already know the bot.
pub async fn menu(state: AppState, msg: Message) -> HandlerResult {
    let Some(user) = reply::require_user(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let outcome =
        screens::show_main(&state, Surface::New(msg.chat.id), user.role == Role::Admin).await;
    reply::finish(&state.bot, msg.chat.id, "menu", outcome).await
}

pub async fn help(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = screens::show_help(&state, Surface::New(msg.chat.id)).await;
    reply::finish(&state.bot, msg.chat.id, "help", outcome).await
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

    // A pasted address is the most likely thing someone sends without a command, so
    // point at the two ways to track one instead of the manual.
    let looks_like_an_address = msg
        .text()
        .map(str::trim)
        .is_some_and(crate::providers::solana::is_valid_address);

    let text = if looks_like_an_address {
        copy::PASTED_AN_ADDRESS
    } else {
        copy::NOT_A_COMMAND
    };

    reply::send_text(&state.bot, msg.chat.id, text).await?;
    Ok(())
}

/// Refuses group, supergroup, and channel chats.
///
/// Replies only to an apparent command attempt: answering every message in a group the
/// bot happens to be in would be noise, and could trip Telegram's flood limits.
pub async fn non_private_chat(state: AppState, msg: Message) -> HandlerResult {
    if msg.text().is_some_and(|text| text.starts_with('/')) {
        tracing::info!(
            chat_id = msg.chat.id.0,
            "refused a command from a non-private chat"
        );
        reply::try_send(&state.bot, msg.chat.id, copy::NOT_A_PRIVATE_CHAT).await;
    }

    Ok(())
}

/// Parses a positive row id from a command argument, replying with usage on failure.
pub async fn parse_id(state: &AppState, msg: &Message, raw: &str, usage: &str) -> Option<i64> {
    match raw.trim().parse::<i64>() {
        Ok(id) if id > 0 => Some(id),
        _ => {
            reply::try_send(
                &state.bot,
                msg.chat.id,
                format!("{usage}\n\nThe number comes from the listing, e.g. /alerts."),
            )
            .await;
            None
        }
    }
}
