use crate::db::repos::wallets::WalletRepo;
use crate::db::Db;
use crate::telegram::auth;
use std::sync::Arc;
use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;

pub async fn start_add_wallet(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    dialogue: Dialogue<crate::telegram::flows::add_wallet::AddWalletState, InMemStorage<crate::telegram::flows::add_wallet::AddWalletState>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match auth::authorize(&db, &msg).await {
        Ok(_) => {}
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    }

    bot.send_message(msg.chat.id, "Send the Solana wallet address to track.").await?;

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
    match auth::authorize(&db, &msg).await {
        Ok(_) => {}
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    }

    let wallets = WalletRepo::new(&db).list().await?;

    if wallets.is_empty() {
        bot.send_message(msg.chat.id, "No wallets tracked yet. Use /addwallet to add one.")
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

    bot.send_message(msg.chat.id, format!("Tracked wallets:\n{text}"))
        .await?;
    Ok(())
}
