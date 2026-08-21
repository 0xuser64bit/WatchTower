//! Admin user management.
//!
//! Two safeguards that did not exist before: an admin cannot demote or block
//! themselves, and the last active admin cannot be removed. Without them a single
//! `/demote <own id>` permanently locked everyone out of the daemon — there is no
//! other way to grant admin — and blocking every admin silently disabled alert
//! delivery, since active admins are the recipient list.

use crate::app_state::AppState;
use crate::db::repos::users::{Role, UserRepo};
use crate::error::Result;
use crate::telegram::commands::parse_id;
use crate::telegram::flows::HandlerResult;
use crate::telegram::reply;
use teloxide::prelude::*;

const PANEL: &str = "\
Admin panel

/listusers - list users and roles
/addadmin <telegram_id> - grant admin
/demote <telegram_id> - revoke admin
/block <telegram_id> - block a user
/unblock <telegram_id> - unblock a user

Only registered, unblocked users can use the bot at all. Active admins are the
recipients for every alert.";

pub async fn panel(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_admin(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    reply::try_send(&state.bot, msg.chat.id, PANEL).await;
    Ok(())
}

pub async fn list_users(state: AppState, msg: Message) -> HandlerResult {
    let Some(actor) = reply::require_admin(&state.bot, &state.db, &msg).await else {
        return Ok(());
    };

    let outcome = render_users(&state, &msg, actor.telegram_id).await;
    reply::finish(&state.bot, msg.chat.id, "list_users", outcome).await
}

async fn render_users(state: &AppState, msg: &Message, actor_id: i64) -> Result<()> {
    let users = UserRepo::new(&state.db).list().await?;

    let body = users
        .iter()
        .map(|user| {
            let mut flags = vec![user.role.to_string()];
            if user.blocked {
                flags.push("blocked".to_string());
            }
            if user.telegram_id == actor_id {
                flags.push("you".to_string());
            }
            format!("{} — {}", user.telegram_id, flags.join(", "))
        })
        .collect::<Vec<_>>()
        .join("\n");

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("Users ({}):\n\n{body}", users.len()),
    )
    .await?;
    Ok(())
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
    if would_remove_last_admin(state, target, msg).await? {
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
    if blocked && would_remove_last_admin(state, target, msg).await? {
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

/// Replies and returns `true` when the action would leave zero active admins.
async fn would_remove_last_admin(state: &AppState, target: i64, msg: &Message) -> Result<bool> {
    let repo = UserRepo::new(&state.db);

    let Some(user) = repo.find_by_telegram_id(target).await? else {
        // Not registered: nothing to remove, and the repo call will report NotFound.
        return Ok(false);
    };

    if user.role != Role::Admin || user.blocked {
        return Ok(false);
    }

    if repo.count_active_admins().await? > 1 {
        return Ok(false);
    }

    reply::try_send(
        &state.bot,
        msg.chat.id,
        "That is the last active admin. Grant admin to someone else first, otherwise \
         nobody could manage the bot or receive alerts.",
    )
    .await;

    Ok(true)
}
