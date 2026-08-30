//! Callback-query routing: the tap-driven half of the control plane.
//!
//! Every inline button carries a compact `domain:action:arg` string in its callback
//! data. This module parses that, re-authorizes the tapper (so a block takes effect
//! between taps exactly as it does between messages), and dispatches to a screen
//! renderer or a guided-flow step.
//!
//! Two rules keep the interface honest under real conditions:
//!
//! * **Every callback is answered.** An unanswered callback leaves a spinner on the
//!   button forever, so [`ui::ack`] runs on the way out of every path.
//! * **A tap edits the message it came from.** Navigation never appends a new bubble,
//!   so the chat does not fill up as the user moves around. Stale taps on a message
//!   whose flow has moved on are caught by the individual handlers and answered with a
//!   short notice rather than acted on.

use crate::app_state::AppState;
use crate::db::repos::users::{AuthUser, Role};
use crate::telegram::auth::{authorize_id, Authorization};
use crate::telegram::flows::{DialogueState, FlowDialogue, HandlerResult};
use crate::telegram::ui::Surface;
use crate::telegram::{flows, reply, screens, ui};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;

// Screen / navigation targets. Kept short because Telegram caps callback data at 64
// bytes and several carry a trailing id.
pub const MAIN: &str = "m";
pub const CANCEL: &str = "x";
pub const NOOP: &str = "noop";
/// The favourites screen. Named because it is referenced from three places — the main
/// menu, the alert flow, and its own pager — and a typo in a literal would silently
/// route to the "button has expired" arm.
pub const FAVOURITES: &str = "fv";

pub fn handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    Update::filter_callback_query().endpoint(handle)
}

/// Resolves the surface a callback should render onto.
///
/// The button normally lives on an editable message we replace in place. If that
/// message is gone or inaccessible (older than 48h), we fall back to a fresh message
/// in the user's private chat, whose id equals their user id.
fn surface_of(q: &CallbackQuery) -> Option<Surface> {
    match &q.message {
        Some(message) => {
            let chat = message.chat();
            // Inline keyboards are only ever sent to private chats, but a callback
            // could still arrive from elsewhere; refuse to act outside a DM.
            if !chat.is_private() {
                return None;
            }
            Some(Surface::Edit(chat.id, message.id()))
        }
        None => Some(Surface::New(ChatId(q.from.id.0 as i64))),
    }
}

async fn handle(
    state: AppState,
    dialogue: FlowDialogue,
    current: DialogueState,
    q: CallbackQuery,
) -> HandlerResult {
    let data = q.data.clone().unwrap_or_default();

    let Some(surface) = surface_of(&q) else {
        ui::ack(&state.bot, q.id).await;
        return Ok(());
    };

    let telegram_id = q.from.id.0 as i64;

    match authorize_id(&state.db, telegram_id).await {
        Ok(Authorization::Allowed(user)) => {
            let outcome = route(&state, &dialogue, current, surface, &q, user, &data).await;
            if let Err(err) = outcome {
                reply::report_error(&state.bot, surface.chat(), "callback", &err).await;
            }
        }
        Ok(Authorization::Denied(reason)) => {
            ui::alert(&state.bot, q.id.clone(), reason.user_message()).await;
        }
        Err(err) => {
            tracing::error!(%err, "callback authorization failed");
            ui::alert(
                &state.bot,
                q.id.clone(),
                "Could not verify your access right now. Please try again shortly.",
            )
            .await;
        }
    }

    // Always clear the spinner. If an alert/toast already answered the query this is a
    // no-op that Telegram rejects, which is fine.
    ui::ack(&state.bot, q.id).await;
    Ok(())
}

/// The routing table. Each arm either renders a screen or advances a flow.
#[allow(clippy::too_many_arguments)]
async fn route(
    state: &AppState,
    dialogue: &FlowDialogue,
    current: DialogueState,
    surface: Surface,
    q: &CallbackQuery,
    user: AuthUser,
    data: &str,
) -> crate::error::Result<()> {
    let parts: Vec<&str> = data.split(':').collect();
    let is_admin = user.role == Role::Admin;

    match parts.as_slice() {
        // ── Navigation ────────────────────────────────────────────────────────
        [MAIN] => screens::show_main(state, surface, is_admin).await,
        [NOOP] => Ok(()),
        [CANCEL] => {
            flows::reset(dialogue).await;
            screens::show_main(state, surface, is_admin).await
        }
        ["hl"] => screens::show_help(state, surface).await,
        ["st"] => screens::show_status(state, surface).await,

        // ── Alerts ────────────────────────────────────────────────────────────
        ["al"] => nav_reset(dialogue, || screens::show_alerts(state, surface, 0)).await,
        ["al", "p", n] => screens::show_alerts(state, surface, page(n)).await,
        ["al", "v", id] => screens::show_alert(state, surface, parse_id(id)?).await,
        ["al", "t", id] => screens::toggle_alert(state, surface, parse_id(id)?).await,
        ["al", "d", id] => screens::confirm_delete_alert(state, surface, parse_id(id)?).await,
        ["al", "dy", id] => screens::delete_alert(state, surface, parse_id(id)?).await,

        // ── Tokens ────────────────────────────────────────────────────────────
        ["tk"] => nav_reset(dialogue, || screens::show_tokens(state, surface, 0)).await,
        ["tk", "p", n] => screens::show_tokens(state, surface, page(n)).await,
        ["tk", "v", id] => screens::show_token(state, surface, parse_id(id)?).await,
        // Starring is a toggle, but the button carries the intended end state rather
        // than "flip it": two taps on a stale keyboard then converge instead of
        // undoing each other.
        ["tk", "f", id, flag] => {
            screens::set_token_favourite(state, surface, parse_id(id)?, parse_flag(flag)?).await
        }
        ["tk", "d", id] => screens::confirm_delete_token(state, surface, parse_id(id)?).await,
        ["tk", "dy", id] => screens::delete_token(state, surface, parse_id(id)?).await,

        // ── Favourites ────────────────────────────────────────────────────────
        [FAVOURITES] => nav_reset(dialogue, || screens::show_favourites(state, surface, 0)).await,
        [FAVOURITES, "p", n] => screens::show_favourites(state, surface, page(n)).await,

        // ── Wallets ───────────────────────────────────────────────────────────
        ["wl"] => nav_reset(dialogue, || screens::show_wallets(state, surface, 0)).await,
        ["wl", "p", n] => screens::show_wallets(state, surface, page(n)).await,
        ["wl", "v", id] => screens::show_wallet(state, surface, parse_id(id)?).await,
        ["wl", "d", id] => screens::confirm_delete_wallet(state, surface, parse_id(id)?).await,
        ["wl", "dy", id] => screens::delete_wallet(state, surface, parse_id(id)?).await,

        // ── History ───────────────────────────────────────────────────────────
        ["hi"] => screens::show_history(state, surface, 0).await,
        ["hi", "p", n] => screens::show_history(state, surface, page(n)).await,

        // ── Guided creation flows ─────────────────────────────────────────────
        ["ac", "new"] => {
            flows::reset(dialogue).await;
            flows::add_alert::start_on(state, dialogue, surface).await
        }
        // "Create Alert" on a token: enters the flow with that token already chosen.
        // An entry point rather than an in-flow step, so it is routed here and does not
        // consult the dialogue.
        ["ac", "tk", id] => {
            flows::add_alert::start_on_token(state, dialogue, surface, parse_id(id)?).await
        }
        ["at", "new"] => {
            flows::reset(dialogue).await;
            flows::add_token::start_on(state, dialogue, surface).await
        }
        ["aw", "new"] => {
            flows::reset(dialogue).await;
            flows::add_wallet::start_on(state, dialogue, surface).await
        }
        // In-flow choices depend on the accumulated dialogue state.
        [domain @ ("ac" | "at" | "aw"), rest @ ..] => {
            flows::on_callback(state, dialogue, current, surface, q, domain, rest).await
        }

        // ── Admin ─────────────────────────────────────────────────────────────
        ["ad", rest @ ..] if is_admin => {
            admin_route(state, dialogue, current, surface, q, user, rest).await
        }
        ["ad", ..] => {
            ui::alert(
                &state.bot,
                q.id.clone(),
                "This action requires admin privileges.",
            )
            .await;
            Ok(())
        }

        _ => {
            ui::toast(&state.bot, q.id.clone(), "That button has expired.").await;
            screens::show_main(state, surface, is_admin).await
        }
    }
}

async fn admin_route(
    state: &AppState,
    dialogue: &FlowDialogue,
    current: DialogueState,
    surface: Surface,
    q: &CallbackQuery,
    actor: AuthUser,
    rest: &[&str],
) -> crate::error::Result<()> {
    match rest {
        [] => screens::show_admin(state, surface).await,
        ["u"] => screens::show_users(state, surface, 0, actor.telegram_id).await,
        ["u", "p", n] => screens::show_users(state, surface, page(n), actor.telegram_id).await,
        ["v", id] => screens::show_user(state, surface, parse_i64(id)?, actor.telegram_id).await,
        ["add"] => {
            flows::reset(dialogue).await;
            flows::add_admin::start_on(state, dialogue, surface).await
        }
        // Confirmation tap for the guided add-admin flow, which needs the pending id
        // held in the dialogue state.
        ["addok"] => {
            flows::add_admin::on_callback(state, dialogue, current, surface, q, rest).await
        }
        // Granting access and unblocking are non-destructive: apply immediately.
        ["pr", id] => {
            screens::promote_user(state, surface, parse_i64(id)?, actor.telegram_id).await
        }
        ["ub", id] => {
            screens::unblock_user(state, surface, parse_i64(id)?, actor.telegram_id).await
        }
        // Removing access is destructive: confirm, then apply.
        ["dm", id] => screens::confirm_demote(state, surface, parse_i64(id)?).await,
        ["dmy", id] => {
            screens::demote_user(state, surface, parse_i64(id)?, actor.telegram_id).await
        }
        ["bl", id] => screens::confirm_block(state, surface, parse_i64(id)?).await,
        ["bly", id] => screens::block_user(state, surface, parse_i64(id)?, actor.telegram_id).await,
        _ => {
            ui::toast(&state.bot, q.id.clone(), "That button has expired.").await;
            screens::show_admin(state, surface).await
        }
    }
}

/// Runs a navigation render after clearing any half-finished flow, mirroring how a
/// slash command abandons an active dialogue.
async fn nav_reset<F, Fut>(dialogue: &FlowDialogue, render: F) -> crate::error::Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = crate::error::Result<()>>,
{
    flows::reset(dialogue).await;
    render().await
}

/// A row id must be a positive integer; anything else is a corrupt or forged button.
fn parse_id(raw: &str) -> crate::error::Result<i64> {
    match raw.parse::<i64>() {
        Ok(id) if id > 0 => Ok(id),
        _ => Err(crate::error::AppError::InvalidInput("bad id".into())),
    }
}

/// Telegram ids can be any i64; validated only as parseable.
fn parse_i64(raw: &str) -> crate::error::Result<i64> {
    raw.parse::<i64>()
        .map_err(|_| crate::error::AppError::InvalidInput("bad id".into()))
}

/// A boolean carried in callback data. Only the two forms this crate emits are
/// accepted, so a hand-crafted button cannot coerce something else into `true`.
fn parse_flag(raw: &str) -> crate::error::Result<bool> {
    match raw {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err(crate::error::AppError::InvalidInput("bad flag".into())),
    }
}

fn page(raw: &str) -> usize {
    raw.parse::<usize>().unwrap_or(0)
}
