//! Guided multi-step flows.
//!
//! All flows share **one** dialogue state whose default is [`DialogueState::Idle`].
//!
//! This is the fix for the defect that made the bot unusable: previously each flow
//! had its own `InMemStorage`, and each state enum defaulted to its first *active*
//! step (`AwaitingMint`, `AwaitingKind`, `AwaitingAddress`). Because
//! `dialogue::enter` inserts `Default::default()` for a chat with no stored state,
//! every user started out in `AwaitingMint`, and the add-token branch — registered
//! before the command branch — matched every incoming message. `/start` and every
//! other command were answered with "that does not look like a valid Solana mint
//! address". With a single storage and an explicit idle default, no flow branch can
//! match unless the user actually started that flow.

pub mod add_alert;
pub mod add_token;
pub mod add_wallet;

use crate::app_state::AppState;
use crate::telegram::reply;
use teloxide::dispatching::dialogue::{Dialogue, InMemStorage};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;

pub type FlowDialogue = Dialogue<DialogueState, InMemStorage<DialogueState>>;
pub type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

/// Which guided flow, if any, a chat is currently in.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum DialogueState {
    /// No flow in progress. Must remain the default: it is what keeps ordinary
    /// messages and commands out of the flow branches.
    #[default]
    Idle,
    AddToken(add_token::Step),
    AddWallet(add_wallet::Step),
    AddAlert(add_alert::Step),
}

impl From<add_token::Step> for DialogueState {
    fn from(step: add_token::Step) -> Self {
        DialogueState::AddToken(step)
    }
}

impl From<add_wallet::Step> for DialogueState {
    fn from(step: add_wallet::Step) -> Self {
        DialogueState::AddWallet(step)
    }
}

impl From<add_alert::Step> for DialogueState {
    fn from(step: add_alert::Step) -> Self {
        DialogueState::AddAlert(step)
    }
}

/// Handles the next message in an active flow. Never matches when idle.
pub fn handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    Update::filter_message()
        .branch(case![DialogueState::AddToken(step)].branch(add_token::handler()))
        .branch(case![DialogueState::AddWallet(step)].branch(add_wallet::handler()))
        .branch(case![DialogueState::AddAlert(step)].branch(add_alert::handler()))
}

/// Shared "reply and stop" used when a flow step receives something unusable. The
/// step stays where it is, so the user can simply answer again.
pub async fn reprompt(state: &AppState, msg: &Message, text: &str) -> crate::error::Result<()> {
    reply::send_text(&state.bot, msg.chat.id, text).await?;
    Ok(())
}

/// Abandons any flow in progress.
///
/// Reads the flow from the injected pre-reset state rather than from storage: the
/// command branch has already cleared the dialogue by the time this runs, and
/// `InMemStorage::remove_dialogue` reports "row not found" if asked to clear it twice.
pub async fn cancel(state: AppState, current: DialogueState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let text = match current {
        DialogueState::Idle => "Nothing to cancel.",
        DialogueState::AddToken(_) => "Cancelled adding a token.",
        DialogueState::AddWallet(_) => "Cancelled adding a wallet.",
        DialogueState::AddAlert(_) => "Cancelled creating an alert.",
    };

    reply::send_text(&state.bot, msg.chat.id, text).await?;
    Ok(())
}

/// Moves a flow to its next step.
///
/// Wraps the storage error so flow bodies can use `?` uniformly and any failure is
/// reported to the user by the handler boundary rather than vanishing into a log.
pub async fn advance<S>(dialogue: &FlowDialogue, step: S) -> crate::error::Result<()>
where
    DialogueState: From<S>,
{
    dialogue
        .update(step)
        .await
        .map_err(|err| crate::error::AppError::Internal(format!("dialogue storage: {err}")))
}

/// Clears a dialogue, tolerating the case where it is already absent.
pub async fn reset(dialogue: &FlowDialogue) {
    if let Err(err) = dialogue.exit().await {
        tracing::debug!(?err, "dialogue was already cleared");
    }
}

/// Text of an incoming message, trimmed. `None` for stickers, photos, and the like.
pub fn text_of(msg: &Message) -> Option<&str> {
    msg.text().map(str::trim).filter(|text| !text.is_empty())
}

/// Interprets an answer to an optional question.
///
/// Accepts the words people actually type as well as the terse `-`, which nobody
/// guesses on their own.
pub fn optional_answer(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "-" | "" | "skip" | "none" | "no" | "n/a" => None,
        _ => Some(raw.trim().to_string()),
    }
}

/// Whether a confirmation step should proceed.
///
/// Accepts the obvious synonyms rather than one exact magic word, so a user replying
/// "y" or "confirm" does not silently discard the work they just did.
pub fn is_affirmative(answer: Option<&str>) -> bool {
    matches!(
        answer.map(|text| text.to_ascii_lowercase()).as_deref(),
        Some("yes" | "y" | "confirm" | "ok" | "okay")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_idle() {
        // The single most important invariant in this module: if this ever changes,
        // flow branches start swallowing unrelated messages again.
        assert_eq!(DialogueState::default(), DialogueState::Idle);
    }

    #[test]
    fn confirmation_accepts_common_synonyms() {
        for answer in ["yes", "Y", "confirm", "OK", "okay"] {
            assert!(is_affirmative(Some(answer)), "{answer}");
        }
        for answer in ["no", "n", "cancel", "maybe", ""] {
            assert!(!is_affirmative(Some(answer)), "{answer}");
        }
        assert!(!is_affirmative(None));
    }

    #[test]
    fn optional_questions_accept_words_as_well_as_a_dash() {
        for skip in ["-", "", "skip", "Skip", "none", "NONE", "no"] {
            assert_eq!(optional_answer(skip), None, "{skip:?}");
        }
        assert_eq!(optional_answer("Treasury"), Some("Treasury".to_string()));
        assert_eq!(optional_answer("  USDC  "), Some("USDC".to_string()));
    }
}
