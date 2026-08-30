//! Alert-rule slash commands.
//!
//! Listing and history open the redesigned screens; the id-taking shortcuts
//! (`/enablerule 3`, `/deleterule 3`) stay for people who prefer typing. The tap-driven
//! path lives in [`crate::telegram::screens`].

use crate::app_state::AppState;
use crate::db::repos::rules::RuleRepo;
use crate::error::Result;
use crate::telegram::commands::parse_id;
use crate::telegram::flows::HandlerResult;
use crate::telegram::ui::Surface;
use crate::telegram::{reply, screens};
use teloxide::prelude::*;

pub async fn list_rules(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = screens::show_alerts(&state, Surface::New(msg.chat.id), 0).await;
    reply::finish(&state.bot, msg.chat.id, "list_rules", outcome).await
}

pub async fn set_enabled(
    state: AppState,
    msg: Message,
    args: String,
    enabled: bool,
) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let command = if enabled { "enablerule" } else { "disablerule" };
    let Some(id) = parse_id(&state, &msg, &args, &format!("/{command} <id>")).await else {
        return Ok(());
    };

    let outcome = toggle_rule(&state, &msg, id, enabled).await;
    reply::finish(&state.bot, msg.chat.id, "set_rule_enabled", outcome).await
}

async fn toggle_rule(state: &AppState, msg: &Message, id: i64, enabled: bool) -> Result<()> {
    let rule = RuleRepo::new(&state.db).set_enabled(id, enabled).await?;

    let verb = if enabled { "enabled" } else { "disabled" };
    let note = if enabled && rule.operator.is_percentage() {
        " Its baseline will be taken from the next observation."
    } else {
        ""
    };

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "Alert #{} {verb} — {} {}.{note}",
            rule.id,
            rule.target.name(),
            rule.condition()
        ),
    )
    .await?;
    Ok(())
}

pub async fn delete_rule(state: AppState, msg: Message, args: String) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let Some(id) = parse_id(&state, &msg, &args, "/deleterule <id>").await else {
        return Ok(());
    };

    let outcome = remove_rule(&state, &msg, id).await;
    reply::finish(&state.bot, msg.chat.id, "delete_rule", outcome).await
}

async fn remove_rule(state: &AppState, msg: &Message, id: i64) -> Result<()> {
    let repo = RuleRepo::new(&state.db);

    let Some(rule) = repo.find(id).await? else {
        reply::send_text(&state.bot, msg.chat.id, format!("No alert with id {id}.")).await?;
        return Ok(());
    };

    repo.delete(id).await?;

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "Alert #{} deleted — {} {}. Past firings stay in /history.",
            rule.id,
            rule.target.name(),
            rule.condition()
        ),
    )
    .await?;
    Ok(())
}

pub async fn history(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = screens::show_history(&state, Surface::New(msg.chat.id), 0).await;
    reply::finish(&state.bot, msg.chat.id, "history", outcome).await
}
