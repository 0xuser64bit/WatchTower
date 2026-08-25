//! Token and wallet slash commands.
//!
//! Listings open the redesigned screens; `/deletetoken <id>` and `/deletewallet <id>`
//! remain as typed shortcuts. The tap-driven path lives in
//! [`crate::telegram::screens`].

use crate::app_state::AppState;
use crate::db::repos::tokens::TokenRepo;
use crate::db::repos::wallets::WalletRepo;
use crate::error::Result;
use crate::telegram::commands::parse_id;
use crate::telegram::flows::HandlerResult;
use crate::telegram::ui::Surface;
use crate::telegram::{reply, screens};
use teloxide::prelude::*;

pub async fn list_tokens(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = screens::show_tokens(&state, Surface::New(msg.chat.id), 0).await;
    reply::finish(&state.bot, msg.chat.id, "list_tokens", outcome).await
}

pub async fn delete_token(state: AppState, msg: Message, args: String) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let Some(id) = parse_id(&state, &msg, &args, "/deletetoken <id>").await else {
        return Ok(());
    };

    let outcome = remove_token(&state, &msg, id).await;
    reply::finish(&state.bot, msg.chat.id, "delete_token", outcome).await
}

async fn remove_token(state: &AppState, msg: &Message, id: i64) -> Result<()> {
    let repo = TokenRepo::new(&state.db);

    let Some(token) = repo.find(id).await? else {
        reply::send_text(&state.bot, msg.chat.id, format!("No token with id {id}.")).await?;
        return Ok(());
    };

    let rules_removed = repo.delete(id).await?;

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "Removed token {}{}.",
            token.display(),
            cascade_note(rules_removed)
        ),
    )
    .await?;
    Ok(())
}

pub async fn list_wallets(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = screens::show_wallets(&state, Surface::New(msg.chat.id), 0).await;
    reply::finish(&state.bot, msg.chat.id, "list_wallets", outcome).await
}

pub async fn delete_wallet(state: AppState, msg: Message, args: String) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let Some(id) = parse_id(&state, &msg, &args, "/deletewallet <id>").await else {
        return Ok(());
    };

    let outcome = remove_wallet(&state, &msg, id).await;
    reply::finish(&state.bot, msg.chat.id, "delete_wallet", outcome).await
}

async fn remove_wallet(state: &AppState, msg: &Message, id: i64) -> Result<()> {
    let repo = WalletRepo::new(&state.db);

    let Some(wallet) = repo.find(id).await? else {
        reply::send_text(&state.bot, msg.chat.id, format!("No wallet with id {id}.")).await?;
        return Ok(());
    };

    let rules_removed = repo.delete(id).await?;

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "Removed wallet {}{}.",
            wallet.display(),
            cascade_note(rules_removed)
        ),
    )
    .await?;
    Ok(())
}

/// Cascading deletes are reported explicitly: silently removing a user's alert rules
/// is exactly the kind of surprise that erodes trust in a monitoring tool.
fn cascade_note(rules_removed: i64) -> String {
    match rules_removed {
        0 => String::new(),
        1 => " and 1 alert rule".to_string(),
        n => format!(" and {n} alert rules"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_note_reads_naturally() {
        assert_eq!(cascade_note(0), "");
        assert_eq!(cascade_note(1), " and 1 alert rule");
        assert_eq!(cascade_note(4), " and 4 alert rules");
    }
}
