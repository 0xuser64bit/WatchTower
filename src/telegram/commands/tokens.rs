use crate::db::repos::tokens::TokenRepo;
use crate::db::Db;
use crate::telegram::reply;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;

pub async fn start_add_token(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    dialogue: Dialogue<
        crate::telegram::flows::add_token::AddTokenState,
        InMemStorage<crate::telegram::flows::add_token::AddTokenState>,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_user(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    bot.send_message(
        msg.chat.id,
        "Send the Solana mint address of the token you want to track.",
    )
    .await?;

    dialogue
        .update(crate::telegram::flows::add_token::AddTokenState::AwaitingMint)
        .await?;

    Ok(())
}

pub async fn list_tokens(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_user(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let tokens = TokenRepo::new(&db).list().await?;

    if tokens.is_empty() {
        bot.send_message(
            msg.chat.id,
            "No tokens tracked yet. Use /addtoken to add one.",
        )
        .await?;
        return Ok(());
    }

    let text = tokens
        .iter()
        .map(|t| {
            format!(
                "{}. {} ({})",
                t.id,
                t.symbol.as_deref().unwrap_or("unknown"),
                t.mint_address
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    reply::send_text(&bot, msg.chat.id, format!("Tracked tokens:\n{text}")).await?;
    Ok(())
}

pub async fn delete_token(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    args: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_user(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let id = match args.trim().parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => {
            bot.send_message(msg.chat.id, "Usage: /deletetoken <id>")
                .await?;
            return Ok(());
        }
    };

    match TokenRepo::new(&db).soft_delete(id).await {
        Ok(()) => {
            bot.send_message(msg.chat.id, "Token deleted.").await?;
        }
        Err(_) => {
            bot.send_message(msg.chat.id, "Token not found.").await?;
        }
    }

    Ok(())
}
