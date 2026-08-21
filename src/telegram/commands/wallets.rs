use crate::db::repos::wallets::WalletRepo;
use crate::db::Db;
use crate::telegram::reply;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;

pub async fn start_add_wallet(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    dialogue: Dialogue<
        crate::telegram::flows::add_wallet::AddWalletState,
        InMemStorage<crate::telegram::flows::add_wallet::AddWalletState>,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_user(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    bot.send_message(msg.chat.id, "Send the Solana wallet address to track.")
        .await?;

    dialogue
        .update(crate::telegram::flows::add_wallet::AddWalletState::AwaitingAddress)
        .await?;

    Ok(())
}

pub async fn list_wallets(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if reply::require_user(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let wallets = WalletRepo::new(&db).list().await?;

    if wallets.is_empty() {
        bot.send_message(
            msg.chat.id,
            "No wallets tracked yet. Use /addwallet to add one.",
        )
        .await?;
        return Ok(());
    }

    let text = wallets
        .iter()
        .map(|w| {
            format!(
                "{}. {} ({})",
                w.id,
                w.label.as_deref().unwrap_or("unlabeled"),
                w.address
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    reply::send_text(&bot, msg.chat.id, format!("Tracked wallets:\n{text}")).await?;
    Ok(())
}

pub async fn delete_wallet(
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
            bot.send_message(msg.chat.id, "Usage: /deletewallet <id>")
                .await?;
            return Ok(());
        }
    };

    match WalletRepo::new(&db).soft_delete(id).await {
        Ok(()) => {
            bot.send_message(msg.chat.id, "Wallet deleted.").await?;
        }
        Err(_) => {
            bot.send_message(msg.chat.id, "Wallet not found.").await?;
        }
    }

    Ok(())
}
