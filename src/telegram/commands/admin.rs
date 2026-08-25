//! Admin user management.
//!
//! The tap-driven admin area lives in [`crate::telegram::screens`]; this module keeps
//! the equivalent slash commands and the shared safeguards. Two invariants are enforced
//! wherever a change is applied, screen or command: an admin cannot demote or block
//! themselves, and the last active admin cannot be removed. Without them a single
//! `/demote <own id>` permanently locked everyone out, and blocking every admin
//! silently disabled alert delivery.

use crate::app_state::AppState;
use crate::db::repos::users::{Role, UserRepo};
use crate::db::Db;
use crate::error::Result;
use crate::telegram::commands::parse_id;
use crate::telegram::flows::HandlerResult;
use crate::telegram::ui::Surface;
use crate::telegram::{menu, reply, screens};
use teloxide::prelude::*;

pub async fn panel(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_admin(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = screens::show_admin(&state, Surface::New(msg.chat.id)).await;
    reply::finish(&state.bot, msg.chat.id, "admin_panel", outcome).await
}

pub async fn list_users(state: AppState, msg: Message) -> HandlerResult {
    let Some(actor) = reply::require_admin(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let outcome =
        screens::show_users(&state, Surface::New(msg.chat.id), 0, actor.telegram_id).await;
    reply::finish(&state.bot, msg.chat.id, "list_users", outcome).await
}

pub async fn add_admin(state: AppState, msg: Message, args: String) -> HandlerResult {
    if reply::require_admin(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let Some(target) = parse_id(&state, &msg, &args, "/addadmin <telegram_id>").await else {
        return Ok(());
    };

    let outcome = promote(&state, &msg, target).await;
    reply::finish(&state.bot, msg.chat.id, "add_admin", outcome).await
}

async fn promote(state: &AppState, msg: &Message, target: i64) -> Result<()> {
    let user = UserRepo::new(&state.db).upsert(target, Role::Admin).await?;

    // So the new admin's command menu gains the admin entries without waiting for a
    // restart.
    menu::publish_for_admin(&state.bot, target, true).await;

    let note = if user.blocked {
        " They are still blocked — use /unblock to restore access."
    } else {
        ""
    };

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("{target} is now an admin.{note}"),
    )
    .await?;
    Ok(())
}

pub async fn demote(state: AppState, msg: Message, args: String) -> HandlerResult {
    let Some(actor) = reply::require_admin(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let Some(target) = parse_id(&state, &msg, &args, "/demote <telegram_id>").await else {
        return Ok(());
    };

    if target == actor.telegram_id {
        reply::send_text(
            &state.bot,
            msg.chat.id,
            "You cannot demote yourself. Ask another admin to do it.",
        )
        .await?;
        return Ok(());
    }

    let outcome = revoke_admin(&state, &msg, target).await;
    reply::finish(&state.bot, msg.chat.id, "demote", outcome).await
}

async fn revoke_admin(state: &AppState, msg: &Message, target: i64) -> Result<()> {
    if would_orphan_admins(&state.db, target).await? {
        reply::try_send(&state.bot, msg.chat.id, LAST_ADMIN_MESSAGE).await;
        return Ok(());
    }

    UserRepo::new(&state.db)
        .set_role(target, Role::User)
        .await?;

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("{target} is no longer an admin and will stop receiving alerts."),
    )
    .await?;
    Ok(())
}

pub async fn set_blocked(
    state: AppState,
    msg: Message,
    args: String,
    blocked: bool,
) -> HandlerResult {
    let Some(actor) = reply::require_admin(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let command = if blocked { "block" } else { "unblock" };
    let Some(target) = parse_id(&state, &msg, &args, &format!("/{command} <telegram_id>")).await
    else {
        return Ok(());
    };

    if blocked && target == actor.telegram_id {
        reply::send_text(&state.bot, msg.chat.id, "You cannot block yourself.").await?;
        return Ok(());
    }

    let outcome = apply_block(&state, &msg, target, blocked).await;
    let context = if blocked {
        "block_user"
    } else {
        "unblock_user"
    };
    reply::finish(&state.bot, msg.chat.id, context, outcome).await
}

async fn apply_block(state: &AppState, msg: &Message, target: i64, blocked: bool) -> Result<()> {
    if blocked && would_orphan_admins(&state.db, target).await? {
        reply::try_send(&state.bot, msg.chat.id, LAST_ADMIN_MESSAGE).await;
        return Ok(());
    }

    UserRepo::new(&state.db)
        .set_blocked(target, blocked)
        .await?;

    let text = if blocked {
        format!("{target} is blocked and will no longer receive alerts.")
    } else {
        format!("{target} is unblocked.")
    };

    reply::send_text(&state.bot, msg.chat.id, text).await?;
    Ok(())
}

const LAST_ADMIN_MESSAGE: &str =
    "That is the last active admin. Grant admin to someone else first, otherwise \
     nobody could manage the bot or receive alerts.";

/// Whether removing (demoting or blocking) `target` would leave zero active admins.
///
/// Shared by the screen and command paths so the guard cannot be bypassed by choosing
/// one interface over the other.
pub async fn would_orphan_admins(db: &Db, target: i64) -> Result<bool> {
    let repo = UserRepo::new(db);

    let Some(user) = repo.find_by_telegram_id(target).await? else {
        // Not registered: nothing to remove.
        return Ok(false);
    };

    if user.role != Role::Admin || user.blocked {
        return Ok(false);
    }

    Ok(repo.count_active_admins().await? <= 1)
}
