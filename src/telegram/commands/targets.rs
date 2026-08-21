//! Listing and deleting tracked tokens and wallets.

use crate::app_state::AppState;
use crate::db::repos::tokens::TokenRepo;
use crate::db::repos::wallets::WalletRepo;
use crate::telegram::commands::parse_id;
use crate::telegram::flows::HandlerResult;
use crate::telegram::reply;
use teloxide::prelude::*;

pub async fn list_tokens(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let tokens = match TokenRepo::new(&state.db).list().await {
        Ok(tokens) => tokens,
        Err(err) => {
            reply::report_error(&state.bot, msg.chat.id, "list_tokens", &err).await;
            return Ok(());
        }
    };

    if tokens.is_empty() {
        reply::send_text(
            &state.bot,
            msg.chat.id,
            "No tokens tracked yet. Use /addtoken to add one.",
        )
        .await?;
        return Ok(());
    }

    let body = tokens
        .iter()
        .map(|token| {
            format!(
                "{}. {} — {}\n   {} alert rule(s)",
                token.id,
                token.symbol.as_deref().unwrap_or("no symbol"),
                token.mint_address,
                token.rule_count
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("Tracked tokens ({}):\n\n{body}", tokens.len()),
    )
    .await?;
    Ok(())
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

    let repo = TokenRepo::new(&state.db);

    let Some(token) = repo.find(id).await? else {
        reply::send_text(&state.bot, msg.chat.id, format!("No token with id {id}.")).await?;
        return Ok(());
    };

    match repo.delete(id).await {
        Ok(rules_removed) => {
            // Cascading deletes are reported explicitly: silently removing a user's
            // alert rules is exactly the kind of surprise that erodes trust.
            let suffix = match rules_removed {
                0 => String::new(),
                1 => " and 1 alert rule".to_string(),
                n => format!(" and {n} alert rules"),
            };

            reply::send_text(
                &state.bot,
                msg.chat.id,
                format!("Removed token {}{suffix}.", token.display()),
            )
            .await?;
        }
        Err(err) => reply::report_error(&state.bot, msg.chat.id, "delete_token", &err).await,
    }

    Ok(())
}

pub async fn list_wallets(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let wallets = match WalletRepo::new(&state.db).list().await {
        Ok(wallets) => wallets,
        Err(err) => {
            reply::report_error(&state.bot, msg.chat.id, "list_wallets", &err).await;
            return Ok(());
        }
    };

    if wallets.is_empty() {
        reply::send_text(
            &state.bot,
            msg.chat.id,
            "No wallets tracked yet. Use /addwallet to add one.",
        )
        .await?;
        return Ok(());
    }

    let body = wallets
        .iter()
        .map(|wallet| {
            format!(
                "{}. {} — {}\n   {} alert rule(s)",
                wallet.id,
                wallet.label.as_deref().unwrap_or("no label"),
                wallet.address,
                wallet.rule_count
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("Tracked wallets ({}):\n\n{body}", wallets.len()),
    )
    .await?;
    Ok(())
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

    let repo = WalletRepo::new(&state.db);

    let Some(wallet) = repo.find(id).await? else {
        reply::send_text(&state.bot, msg.chat.id, format!("No wallet with id {id}.")).await?;
        return Ok(());
    };

    match repo.delete(id).await {
        Ok(rules_removed) => {
            let suffix = match rules_removed {
                0 => String::new(),
                1 => " and 1 alert rule".to_string(),
                n => format!(" and {n} alert rules"),
            };

            reply::send_text(
                &state.bot,
                msg.chat.id,
                format!("Removed wallet {}{suffix}.", wallet.display()),
            )
            .await?;
        }
        Err(err) => reply::report_error(&state.bot, msg.chat.id, "delete_wallet", &err).await,
    }

    Ok(())
}
