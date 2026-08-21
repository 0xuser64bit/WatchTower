//! Guided token tracking.
//!
//! The mint is verified against the price provider before the token is stored. A
//! token with no USD listing can never satisfy a price rule, so accepting it would
//! let the user build an alert that silently never fires.

use crate::app_state::AppState;
use crate::db::repos::tokens::TokenRepo;
use crate::error::Result;
use crate::providers::solana::is_valid_address;
use crate::providers::ProviderError;
use crate::telegram::flows::{optional_answer, reprompt, text_of, FlowDialogue, HandlerResult};
use crate::telegram::reply;
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    AwaitingMint,
    AwaitingSymbol {
        mint: String,
        price: Option<f64>,
    },
    Confirming {
        mint: String,
        symbol: Option<String>,
    },
}

pub fn handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    dptree::entry()
        .branch(case![Step::AwaitingMint].endpoint(await_mint))
        .branch(case![Step::AwaitingSymbol { mint, price }].endpoint(await_symbol))
        .branch(case![Step::Confirming { mint, symbol }].endpoint(confirm))
}

pub async fn start(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = start_body(&state, &dialogue, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "add_token.start", outcome).await
}

async fn start_body(state: &AppState, dialogue: &FlowDialogue, msg: &Message) -> Result<()> {
    reply::send_text(
        &state.bot,
        msg.chat.id,
        "Send the SPL token mint address you want to track.\n\nSend /cancel to stop.",
    )
    .await?;

    super::advance(dialogue, Step::AwaitingMint).await?;
    Ok(())
}

async fn await_mint(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let outcome = await_mint_body(&state, &dialogue, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "add_token.mint", outcome).await
}

async fn await_mint_body(state: &AppState, dialogue: &FlowDialogue, msg: &Message) -> Result<()> {
    let Some(mint) = text_of(msg) else {
        return reprompt(state, msg, "Send the mint address as text.").await;
    };

    if !is_valid_address(mint) {
        return reprompt(
            state,
            msg,
            "That is not a valid Solana address. A mint address is 32-44 base58 characters.",
        )
        .await;
    }

    let mint = mint.to_string();

    if let Some(existing) = TokenRepo::new(&state.db).find_by_mint(&mint).await? {
        reply::send_text(
            &state.bot,
            msg.chat.id,
            format!(
                "Already tracked as token {} ({}). Use /alerts to add a rule for it.",
                existing.id,
                existing.symbol.as_deref().unwrap_or("no symbol")
            ),
        )
        .await?;
        super::reset(dialogue).await;
        return Ok(());
    }

    // Confirm the provider can actually price this mint.
    let price = match state.price_provider.get_token_price_usd(&mint).await {
        Ok(price) => Some(price),
        Err(ProviderError::Unsupported(_)) => {
            reply::send_text(
                &state.bot,
                msg.chat.id,
                "The price provider has no USD listing for that mint, so a price \
                 alert could never fire. Not adding it.",
            )
            .await?;
            super::reset(dialogue).await;
            return Ok(());
        }
        Err(err) => {
            // A transient outage must not block tracking a legitimate token.
            tracing::warn!(%mint, %err, "could not verify token price while adding");
            None
        }
    };

    let intro = match price {
        Some(price) => format!(
            "Current price: {} USD.",
            crate::alerts::format::amount(price)
        ),
        None => "Could not reach the price provider to verify it right now.".to_string(),
    };

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("{intro}\n\nSend a symbol to label it (for example USDC), or `-` to skip."),
    )
    .await?;

    super::advance(dialogue, Step::AwaitingSymbol { mint, price }).await?;
    Ok(())
}

async fn await_symbol(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (mint, _price): (String, Option<f64>),
) -> HandlerResult {
    let outcome = await_symbol_body(&state, &dialogue, &msg, (mint, _price)).await;
    reply::finish(&state.bot, msg.chat.id, "add_token.symbol", outcome).await
}

async fn await_symbol_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
    (mint, _price): (String, Option<f64>),
) -> Result<()> {
    let Some(raw) = text_of(msg) else {
        return reprompt(state, msg, "Send a symbol as text, or `-` to skip.").await;
    };

    let symbol = optional_answer(raw);

    if let Some(symbol) = &symbol {
        if symbol.chars().count() > 32 {
            return reprompt(state, msg, "Keep the symbol to 32 characters or fewer.").await;
        }
    }

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "Track this token?\n\nMint: {mint}\nSymbol: {}\n\nReply `yes` to confirm, or /cancel.",
            symbol.as_deref().unwrap_or("(none)")
        ),
    )
    .await?;

    super::advance(dialogue, Step::Confirming { mint, symbol }).await?;
    Ok(())
}

async fn confirm(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (mint, symbol): (String, Option<String>),
) -> HandlerResult {
    let outcome = confirm_body(&state, &dialogue, &msg, (mint, symbol)).await;
    reply::finish(&state.bot, msg.chat.id, "add_token.confirm", outcome).await
}

async fn confirm_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
    (mint, symbol): (String, Option<String>),
) -> Result<()> {
    if !super::is_affirmative(text_of(msg)) {
        super::reset(dialogue).await;
        return reprompt(state, msg, "Cancelled. Nothing was added.").await;
    }

    match TokenRepo::new(&state.db)
        .create(&mint, symbol.as_deref())
        .await
    {
        Ok(token) => {
            reply::send_text(
                &state.bot,
                msg.chat.id,
                format!(
                    "Tracking token {} — {}.\n\nUse /addalert to create a price alert for it.",
                    token.id,
                    token.display()
                ),
            )
            .await?;
        }
        Err(err) => {
            reply::report_error(&state.bot, msg.chat.id, "add_token", &err).await;
        }
    }

    super::reset(dialogue).await;
    Ok(())
}
