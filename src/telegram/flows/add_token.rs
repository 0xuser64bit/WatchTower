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
    AwaitingSymbol { mint: String, decimals: i64 },
    Confirm { mint: String, decimals: i64, symbol: Option<String> },
}

pub fn message_handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    Update::filter_message()
        .branch(case![AddTokenState::AwaitingMint].endpoint(await_mint))
        .branch(case![AddTokenState::AwaitingSymbol { mint, decimals }].endpoint(await_symbol))
        .branch(case![AddTokenState::Confirm { mint, decimals, symbol }].endpoint(confirm))
}

async fn await_mint(bot: Bot, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let text = match msg.text() {
        Some(text) => text.trim().to_string(),
        None => {
            bot.send_message(msg.chat.id, "Please send a valid Solana mint address.").await?;
            return Ok(());
        }
    };

    if text.len() < 32 || text.len() > 44 {
        bot.send_message(msg.chat.id, "That does not look like a valid Solana mint address.").await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, "Mint received. Optional symbol? Send `-` to skip.").await?;

    dialogue
        .update(AddTokenState::AwaitingSymbol { mint: text, decimals: 0 })
        .await?;
    Ok(())
}

async fn await_symbol(
    bot: Bot,
    dialogue: FlowDialogue,
    msg: Message,
    mint: String,
    decimals: i64,
) -> HandlerResult {
    let text = msg.text().map(|s| s.trim().to_string());
    let symbol = if text.as_deref() == Some("-") {
        None
    } else {
        text
    };

    let symbol_display = symbol.as_deref().unwrap_or("(no symbol)");
    let keyboard = make_confirm_keyboard();

    bot.send_message(
        msg.chat.id,
        format!("Add token?\nMint: {mint}\nDecimals: {decimals}\nSymbol: {symbol_display}"),
    )
    .reply_markup(keyboard)
    .await?;

    dialogue
        .update(AddTokenState::Confirm { mint, decimals, symbol })
        .await?;
    Ok(())
}

async fn confirm(
    bot: Bot,
    dialogue: FlowDialogue,
    db: Arc<Db>,
    msg: Message,
    mint: String,
    decimals: i64,
    symbol: Option<String>,
) -> HandlerResult {
    if msg.text().map(|s| s.to_lowercase()) != Some("confirm".into()) {
        bot.send_message(msg.chat.id, "Cancelled.").await?;
        dialogue.exit().await?;
        return Ok(());
    }

    let repo = crate::db::repos::tokens::TokenRepo::new(&db);

    match repo.create(&mint, symbol.as_deref(), None, decimals).await {
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

fn make_confirm_keyboard() -> teloxide::types::InlineKeyboardMarkup {
    use teloxide::types::InlineKeyboardButton;

    let confirm = InlineKeyboardButton::callback("Confirm", "addtoken_confirm");
    let cancel = InlineKeyboardButton::callback("Cancel", "addtoken_cancel");

    teloxide::types::InlineKeyboardMarkup::new(vec![vec![confirm, cancel]])
}
