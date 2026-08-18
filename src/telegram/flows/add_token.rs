use crate::db::repos::tokens::TokenRepo;
use crate::db::Db;
use std::sync::Arc;
use teloxide::dispatching::dialogue::{Dialogue, InMemStorage};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;

pub type FlowDialogue = Dialogue<AddTokenState, InMemStorage<AddTokenState>>;
type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Default)]
pub enum AddTokenState {
    #[default]
    AwaitingMint,
    AwaitingSymbol { mint: String },
    Confirm { mint: String, symbol: Option<String> },
}

pub fn message_handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    Update::filter_message()
        .branch(case![AddTokenState::AwaitingMint].endpoint(await_mint))
        .branch(case![AddTokenState::AwaitingSymbol { mint }].endpoint(await_symbol))
        .branch(case![AddTokenState::Confirm { mint, symbol }].endpoint(confirm))
}

async fn await_mint(bot: Bot, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let Some(text) = msg.text().map(|s| s.trim().to_string()) else {
        bot.send_message(msg.chat.id, "Please send a valid Solana mint address.").await?;
        return Ok(());
    };

    if !crate::providers::solana::validation::is_valid_base58_address(&text) {
        bot.send_message(msg.chat.id, "That does not look like a valid Solana mint address.").await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, "Mint received. Optional symbol? Send `-` to skip.").await?;

    dialogue
        .update(AddTokenState::AwaitingSymbol { mint: text })
        .await?;
    Ok(())
}

async fn await_symbol(
    bot: Bot,
    dialogue: FlowDialogue,
    msg: Message,
    mint: String,
) -> HandlerResult {
    let text = msg.text().map(|s| s.trim().to_string());
    let symbol = if text.as_deref() == Some("-") {
        None
    } else {
        text
    };

    let symbol_display = symbol.as_deref().unwrap_or("(no symbol)");
    bot.send_message(
        msg.chat.id,
        format!("Add token?\nMint: {mint}\nSymbol: {symbol_display}\n\nReply `confirm` to create or `cancel` to abort."),
    )
    .await?;

    dialogue
        .update(AddTokenState::Confirm { mint, symbol })
        .await?;
    Ok(())
}

async fn confirm(
    bot: Bot,
    dialogue: FlowDialogue,
    db: Arc<Db>,
    msg: Message,
    mint: String,
    symbol: Option<String>,
) -> HandlerResult {
    if msg.text().map(|s| s.trim().to_lowercase()) != Some("confirm".to_string()) {
        bot.send_message(msg.chat.id, "Cancelled.").await?;
        dialogue.exit().await?;
        return Ok(());
    }

    match TokenRepo::new(&db).create(&mint, symbol.as_deref(), None).await {
        Ok(_) => {
            bot.send_message(msg.chat.id, "Token added successfully.").await?;
        }
        Err(_) => {
            bot.send_message(msg.chat.id, "Failed to add token.").await?;
        }
    }

    dialogue.exit().await?;
    Ok(())
}
