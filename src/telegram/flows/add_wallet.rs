//! Guided wallet tracking.
//!
//! The address is checked on chain before the wallet is stored, so the user sees the
//! real balance and immediately notices a mistyped address.

use crate::app_state::AppState;
use crate::db::repos::wallets::WalletRepo;
use crate::providers::solana::is_valid_address;
use crate::telegram::flows::{
    is_affirmative, optional_answer, reprompt, text_of, FlowDialogue, HandlerResult,
};
use crate::telegram::reply;
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;

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

    reply::send_text(
        &state.bot,
        msg.chat.id,
        "Send the Solana wallet address you want to track.\n\nSend /cancel to stop.",
    )
    .await?;

    dialogue.update(Step::AwaitingAddress).await?;
    Ok(())
}

async fn await_address(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let Some(address) = text_of(&msg) else {
        return reprompt(&state, &msg, "Send the wallet address as text.").await;
    };

    if !is_valid_address(address) {
        return reprompt(
            &state,
            &msg,
            "That is not a valid Solana address. Addresses are 32-44 base58 characters.",
        )
        .await;
    }

    let address = address.to_string();

    if let Some(existing) = WalletRepo::new(&state.db).find_by_address(&address).await? {
        reply::send_text(
            &state.bot,
            msg.chat.id,
            format!(
                "Already tracked as wallet {} ({}).",
                existing.id,
                existing.label.as_deref().unwrap_or("no label")
            ),
        )
        .await?;
        super::reset(&dialogue).await;
        return Ok(());
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
            "Could not reach a Solana RPC endpoint to verify it right now.".to_string()
        }
    };

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("{intro}\n\nSend a label for this wallet, or `-` to skip."),
    )
    .await?;

    dialogue.update(Step::AwaitingLabel { address }).await?;
    Ok(())
}

async fn await_label(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    address: String,
) -> HandlerResult {
    let Some(raw) = text_of(&msg) else {
        return reprompt(&state, &msg, "Send a label as text, or `-` to skip.").await;
    };

    let label = optional_answer(raw);

    if let Some(label) = &label {
        if label.chars().count() > 64 {
            return reprompt(&state, &msg, "Keep the label to 64 characters or fewer.").await;
        }
    }

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "Track this wallet?\n\nAddress: {address}\nLabel: {}\n\nReply `yes` to confirm, or /cancel.",
            label.as_deref().unwrap_or("(none)")
        ),
    )
    .await?;

    dialogue.update(Step::Confirming { address, label }).await?;
    Ok(())
}

async fn confirm(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (address, label): (String, Option<String>),
) -> HandlerResult {
    if !is_affirmative(text_of(&msg)) {
        super::reset(&dialogue).await;
        return reprompt(&state, &msg, "Cancelled. Nothing was added.").await;
    }

    match WalletRepo::new(&state.db)
        .create(&address, label.as_deref())
        .await
    {
        Ok(wallet) => {
            reply::send_text(
                &state.bot,
                msg.chat.id,
                format!(
                    "Tracking wallet {} — {}.\n\nUse /addalert to create a balance alert for it.",
                    wallet.id,
                    wallet.display()
                ),
            )
            .await?;
        }
        Err(err) => reply::report_error(&state.bot, msg.chat.id, "add_wallet", &err).await,
    }

    super::reset(&dialogue).await;
    Ok(())
}
