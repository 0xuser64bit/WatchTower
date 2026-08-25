//! Guided token tracking.
//!
//! The mint is verified against the price provider before the token is stored: a mint
//! with no USD listing can never satisfy a price rule, so accepting it would let the
//! user build an alert that silently never fires. Naming and confirmation are tappable
//! (Skip / Add), while the address itself is pasted as text.

use crate::app_state::AppState;
use crate::db::repos::tokens::TokenRepo;
use crate::error::Result;
use crate::providers::solana::is_valid_address;
use crate::providers::ProviderError;
use crate::telegram::callback::{CANCEL, MAIN};
use crate::telegram::flows::{
    optional_answer, reprompt, text_of, DialogueState, FlowDialogue, HandlerResult,
};
use crate::telegram::ui::{self, button, menu_row, Screen, Surface};
use crate::telegram::{copy, reply};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;
use teloxide::types::CallbackQuery;

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

    let outcome = start_on(&state, &dialogue, Surface::New(msg.chat.id)).await;
    reply::finish(&state.bot, msg.chat.id, "add_token.start", outcome).await
}

pub async fn start_on(state: &AppState, dialogue: &FlowDialogue, surface: Surface) -> Result<()> {
    super::advance(dialogue, Step::AwaitingMint).await?;
    ui::render(
        &state.bot,
        surface,
        Screen::new(copy::ASK_MINT, vec![vec![button("✕ Cancel", CANCEL)]]),
    )
    .await
}

pub async fn on_callback(
    state: &AppState,
    dialogue: &FlowDialogue,
    current: DialogueState,
    surface: Surface,
    q: &CallbackQuery,
    rest: &[&str],
) -> Result<()> {
    match rest {
        ["sk"] => {
            let DialogueState::AddToken(Step::AwaitingSymbol { mint, .. }) = current else {
                return expired(state, q).await;
            };
            present_confirm(state, dialogue, surface, mint, None).await
        }
        ["ok"] => {
            let DialogueState::AddToken(Step::Confirming { mint, symbol }) = current else {
                return expired(state, q).await;
            };
            create(state, dialogue, surface, mint, symbol).await
        }
        _ => expired(state, q).await,
    }
}

async fn expired(state: &AppState, q: &CallbackQuery) -> Result<()> {
    ui::toast(
        &state.bot,
        q.id.clone(),
        "That step has moved on. Send /menu to start over.",
    )
    .await;
    Ok(())
}

async fn await_mint(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let outcome = await_mint_body(&state, &dialogue, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "add_token.mint", outcome).await
}

async fn await_mint_body(state: &AppState, dialogue: &FlowDialogue, msg: &Message) -> Result<()> {
    let Some(mint) = text_of(msg) else {
        return reprompt(state, msg, "Paste the mint address as text.").await;
    };

    if !is_valid_address(mint) {
        return reprompt(state, msg, copy::BAD_ADDRESS).await;
    }

    let mint = mint.to_string();
    let surface = Surface::New(msg.chat.id);

    if let Some(existing) = TokenRepo::new(&state.db).find_by_mint(&mint).await? {
        super::reset(dialogue).await;
        let name = existing.symbol.as_deref().unwrap_or("unnamed");
        return ui::render(
            &state.bot,
            surface,
            Screen::new(
                format!("Already tracking <b>{}</b>.", ui::esc(name)),
                vec![vec![button("🪙 Tokens", "tk"), button("🏠 Menu", MAIN)]],
            ),
        )
        .await;
    }

    // Confirm the provider can actually price this mint.
    let price = match state.price_provider.get_token_price_usd(&mint).await {
        Ok(price) => Some(price),
        Err(ProviderError::Unsupported(_)) => {
            super::reset(dialogue).await;
            return ui::render(
                &state.bot,
                surface,
                Screen::new(copy::NOT_PRICED, vec![menu_row()]),
            )
            .await;
        }
        Err(err) => {
            // A transient outage must not block tracking a legitimate token.
            tracing::warn!(%mint, %err, "could not verify token price while adding");
            None
        }
    };

    present_symbol(state, dialogue, surface, mint, price).await
}

async fn present_symbol(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    mint: String,
    price: Option<f64>,
) -> Result<()> {
    let intro = match price {
        Some(price) => format!(
            "Current price: {} USD.",
            crate::alerts::format::amount(price)
        ),
        None => "Couldn't reach the price provider to verify it right now.".to_string(),
    };

    let rows = vec![vec![button("Skip", "at:sk"), button("✕ Cancel", CANCEL)]];

    super::advance(dialogue, Step::AwaitingSymbol { mint, price }).await?;
    ui::render(
        &state.bot,
        surface,
        Screen::new(copy::ask_token_name(&intro), rows),
    )
    .await
}

async fn await_symbol(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (mint, _price): (String, Option<f64>),
) -> HandlerResult {
    let outcome = await_symbol_body(&state, &dialogue, &msg, mint).await;
    reply::finish(&state.bot, msg.chat.id, "add_token.symbol", outcome).await
}

async fn await_symbol_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
    mint: String,
) -> Result<()> {
    let Some(raw) = text_of(msg) else {
        return reprompt(state, msg, copy::ASK_SHORT_NAME).await;
    };

    let symbol = optional_answer(raw);

    if let Some(symbol) = &symbol {
        if symbol.chars().count() > 32 {
            return reprompt(state, msg, "Keep the symbol to 32 characters or fewer.").await;
        }
    }

    present_confirm(state, dialogue, Surface::New(msg.chat.id), mint, symbol).await
}

async fn present_confirm(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    mint: String,
    symbol: Option<String>,
) -> Result<()> {
    let text = format!(
        concat!(
            "<b>Track this token?</b>\n",
            "\n",
            "<b>Name:</b> {0}\n",
            "<b>Mint:</b>\n{1}"
        ),
        ui::esc(symbol.as_deref().unwrap_or("(none)")),
        ui::code(&mint)
    );
    let rows = vec![
        vec![button("✅ Add Token", "at:ok")],
        vec![button("✕ Cancel", CANCEL)],
    ];

    super::advance(dialogue, Step::Confirming { mint, symbol }).await?;
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

async fn confirm(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (mint, symbol): (String, Option<String>),
) -> HandlerResult {
    let outcome = if super::is_affirmative(text_of(&msg)) {
        create(&state, &dialogue, Surface::New(msg.chat.id), mint, symbol).await
    } else {
        super::reset(&dialogue).await;
        reprompt(&state, &msg, copy::CANCELLED_NOTHING_ADDED).await
    };
    reply::finish(&state.bot, msg.chat.id, "add_token.confirm", outcome).await
}

async fn create(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    mint: String,
    symbol: Option<String>,
) -> Result<()> {
    match TokenRepo::new(&state.db)
        .create(&mint, symbol.as_deref())
        .await
    {
        Ok(token) => {
            let name = token
                .symbol
                .clone()
                .unwrap_or_else(|| "unnamed".to_string());
            let rows = vec![vec![
                button("🚨 Create Alert", "ac:new"),
                button("🏠 Menu", MAIN),
            ]];
            super::reset(dialogue).await;
            ui::render(
                &state.bot,
                surface,
                Screen::new(copy::token_saved(&ui::esc(&name)), rows),
            )
            .await?;
        }
        Err(err) => {
            super::reset(dialogue).await;
            reply::report_error(&state.bot, surface.chat(), "add_token", &err).await;
        }
    }

    Ok(())
}
