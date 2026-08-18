use crate::db::repos::wallets::WalletRepo;
use crate::db::Db;
use std::sync::Arc;
use teloxide::dispatching::dialogue::{Dialogue, InMemStorage};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;

pub type FlowDialogue = Dialogue<AddWalletState, InMemStorage<AddWalletState>>;
type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Default)]
pub enum AddWalletState {
    #[default]
    AwaitingAddress,
    AwaitingLabel {
        address: String,
    },
    Confirm {
        address: String,
        label: Option<String>,
    },
}

pub fn message_handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    Update::filter_message()
        .branch(case![AddWalletState::AwaitingAddress].endpoint(await_address))
        .branch(case![AddWalletState::AwaitingLabel { address }].endpoint(await_label))
        .branch(case![AddWalletState::Confirm { address, label }].endpoint(confirm))
}

async fn await_address(bot: Bot, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let Some(address) = msg.text().map(|s| s.trim().to_string()) else {
        bot.send_message(msg.chat.id, "Please send a valid Solana wallet address.")
            .await?;
        return Ok(());
    };

    if !crate::providers::solana::validation::is_valid_base58_address(&address) {
        bot.send_message(msg.chat.id, "Please send a valid Solana wallet address.")
            .await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, "Optional label? Send `-` to skip.")
        .await?;
    dialogue
        .update(AddWalletState::AwaitingLabel { address })
        .await?;
    Ok(())
}

async fn await_label(
    bot: Bot,
    dialogue: FlowDialogue,
    msg: Message,
    address: String,
) -> HandlerResult {
    let label = msg.text().map(|s| s.trim().to_string());
    let label = if label.as_deref() == Some("-") {
        None
    } else {
        label
    };

    let label_display = label.as_deref().unwrap_or("(no label)");
    bot.send_message(
        msg.chat.id,
        format!("Add wallet?\nAddress: {address}\nLabel: {label_display}\n\nReply `confirm` to create or `cancel` to abort."),
    )
    .await?;

    dialogue
        .update(AddWalletState::Confirm { address, label })
        .await?;
    Ok(())
}

async fn confirm(
    bot: Bot,
    dialogue: FlowDialogue,
    db: Arc<Db>,
    msg: Message,
    address: String,
    label: Option<String>,
) -> HandlerResult {
    let reply = msg.text().map(|s| s.trim().to_lowercase());

    if reply != Some("confirm".to_string()) {
        bot.send_message(msg.chat.id, "Cancelled.").await?;
        dialogue.exit().await?;
        return Ok(());
    }

    match WalletRepo::new(&db)
        .create(&address, label.as_deref())
        .await
    {
        Ok(_) => {
            bot.send_message(msg.chat.id, "Wallet added successfully.")
                .await?;
        }
        Err(_) => {
            bot.send_message(msg.chat.id, "Failed to add wallet.")
                .await?;
        }
    }

    dialogue.exit().await?;
    Ok(())
}
