//! Guided wallet tracking.
//!
//! The address is checked on chain before the wallet is stored, so the user sees the
//! real balance and immediately notices a mistyped address. Naming and confirmation
//! are tappable; the address is pasted as text.

use crate::app_state::AppState;
use crate::db::repos::wallets::WalletRepo;
use crate::error::Result;
use crate::providers::solana::is_valid_address;
use crate::telegram::callback::{CANCEL, MAIN};
use crate::telegram::flows::{
    is_affirmative, optional_answer, reprompt, text_of, DialogueState, FlowDialogue, HandlerResult,
};
use crate::telegram::ui::{self, button, Screen, Surface};
use crate::telegram::{copy, reply};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;
use teloxide::types::CallbackQuery;

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    AwaitingAddress,
    AwaitingLabel {
        address: String,
    },
    Confirming {
        address: String,
        label: Option<String>,
    },
}

pub fn handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    dptree::entry()
        .branch(case![Step::AwaitingAddress].endpoint(await_address))
        .branch(case![Step::AwaitingLabel { address }].endpoint(await_label))
        .branch(case![Step::Confirming { address, label }].endpoint(confirm))
}

pub async fn start(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = start_on(&state, &dialogue, Surface::New(msg.chat.id)).await;
    reply::finish(&state.bot, msg.chat.id, "add_wallet.start", outcome).await
}

pub async fn start_on(state: &AppState, dialogue: &FlowDialogue, surface: Surface) -> Result<()> {
    super::advance(dialogue, Step::AwaitingAddress).await?;
    ui::render(
        &state.bot,
        surface,
        Screen::new(copy::ASK_ADDRESS, vec![vec![button("✕ Cancel", CANCEL)]]),
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
            let DialogueState::AddWallet(Step::AwaitingLabel { address }) = current else {
                return expired(state, q).await;
            };
            present_confirm(state, dialogue, surface, address, None).await
        }
        ["ok"] => {
            let DialogueState::AddWallet(Step::Confirming { address, label }) = current else {
                return expired(state, q).await;
            };
            create(state, dialogue, surface, address, label).await
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

async fn await_address(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let outcome = await_address_body(&state, &dialogue, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "add_wallet.address", outcome).await
}

async fn await_address_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
) -> Result<()> {
    let Some(address) = text_of(msg) else {
        return reprompt(state, msg, "Paste the wallet address as text.").await;
    };

    if !is_valid_address(address) {
        return reprompt(state, msg, copy::BAD_ADDRESS).await;
    }

    let address = address.to_string();
    let surface = Surface::New(msg.chat.id);

    if let Some(existing) = WalletRepo::new(&state.db).find_by_address(&address).await? {
        super::reset(dialogue).await;
        let name = existing.label.as_deref().unwrap_or("unnamed");
        return ui::render(
            &state.bot,
            surface,
            Screen::new(
                format!("Already tracking <b>{}</b>.", ui::esc(name)),
                vec![vec![button("👛 Wallets", "wl"), button("🏠 Menu", MAIN)]],
            ),
        )
        .await;
    }

    let balance = state
        .chain_provider
        .get_native_balance_lamports(&address)
        .await;

    let intro = match balance {
        Ok(lamports) => format!(
            "Current balance: {} SOL.",
            crate::alerts::format::amount(lamports as f64 / LAMPORTS_PER_SOL)
        ),
        Err(err) => {
            tracing::warn!(%address, %err, "could not read wallet balance while adding");
            "Couldn't reach a Solana RPC endpoint to verify it right now.".to_string()
        }
    };

    present_label(state, dialogue, surface, address, intro).await
}

async fn present_label(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    address: String,
    intro: String,
) -> Result<()> {
    let rows = vec![vec![button("Skip", "aw:sk"), button("✕ Cancel", CANCEL)]];

    super::advance(dialogue, Step::AwaitingLabel { address }).await?;
    ui::render(
        &state.bot,
        surface,
        Screen::new(copy::ask_wallet_name(&intro), rows),
    )
    .await
}

async fn await_label(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    address: String,
) -> HandlerResult {
    let outcome = await_label_body(&state, &dialogue, &msg, address).await;
    reply::finish(&state.bot, msg.chat.id, "add_wallet.label", outcome).await
}

async fn await_label_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
    address: String,
) -> Result<()> {
    let Some(raw) = text_of(msg) else {
        return reprompt(state, msg, copy::ASK_SHORT_NAME).await;
    };

    let label = optional_answer(raw);

    if let Some(label) = &label {
        if label.chars().count() > 64 {
            return reprompt(state, msg, "Keep the label to 64 characters or fewer.").await;
        }
    }

    present_confirm(state, dialogue, Surface::New(msg.chat.id), address, label).await
}

async fn present_confirm(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    address: String,
    label: Option<String>,
) -> Result<()> {
    let text = format!(
        concat!(
            "<b>Track this wallet?</b>\n",
            "\n",
            "<b>Name:</b> {0}\n",
            "<b>Address:</b>\n{1}"
        ),
        ui::esc(label.as_deref().unwrap_or("(none)")),
        ui::code(&address)
    );
    let rows = vec![
        vec![button("✅ Add Wallet", "aw:ok")],
        vec![button("✕ Cancel", CANCEL)],
    ];

    super::advance(dialogue, Step::Confirming { address, label }).await?;
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}

async fn confirm(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (address, label): (String, Option<String>),
) -> HandlerResult {
    let outcome = if is_affirmative(text_of(&msg)) {
        create(&state, &dialogue, Surface::New(msg.chat.id), address, label).await
    } else {
        super::reset(&dialogue).await;
        reprompt(&state, &msg, copy::CANCELLED_NOTHING_ADDED).await
    };
    reply::finish(&state.bot, msg.chat.id, "add_wallet.confirm", outcome).await
}

async fn create(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    address: String,
    label: Option<String>,
) -> Result<()> {
    match WalletRepo::new(&state.db)
        .create(&address, label.as_deref())
        .await
    {
        Ok(wallet) => {
            let name = wallet
                .label
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
                Screen::new(copy::wallet_saved(&ui::esc(&name)), rows),
            )
            .await?;
        }
        Err(err) => {
            super::reset(dialogue).await;
            reply::report_error(&state.bot, surface.chat(), "add_wallet", &err).await;
        }
    }

    Ok(())
}
