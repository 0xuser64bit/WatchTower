//! Guided "add an admin" flow.
//!
//! Adding an admin is the one management action that genuinely needs a value the UI
//! cannot offer as a button — a Telegram user id for someone the bot may never have
//! seen. So it is the one guided text step in the admin area: prompt, validate,
//! confirm, apply. Every step re-checks the actor is still an admin, so a demotion
//! mid-flow takes effect immediately.

use crate::app_state::AppState;
use crate::db::repos::users::{Role, UserRepo};
use crate::error::Result;
use crate::telegram::callback::{CANCEL, MAIN};
use crate::telegram::flows::{is_affirmative, text_of, DialogueState, FlowDialogue, HandlerResult};
use crate::telegram::ui::{self, button, Screen, Surface};
use crate::telegram::{copy, menu, reply};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;
use teloxide::types::CallbackQuery;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    AwaitingUserId,
    Confirming { target: i64 },
}

pub fn handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    dptree::entry()
        .branch(case![Step::AwaitingUserId].endpoint(await_user_id))
        .branch(case![Step::Confirming { target }].endpoint(confirm))
}

/// Started by the Admin Panel's "Add Admin" button; the caller has already checked the
/// admin role.
pub async fn start_on(state: &AppState, dialogue: &FlowDialogue, surface: Surface) -> Result<()> {
    super::advance(dialogue, Step::AwaitingUserId).await?;
    ui::render(
        &state.bot,
        surface,
        Screen::new(copy::ask_admin_id(), vec![vec![button("✕ Cancel", CANCEL)]]),
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
        ["addok"] => {
            let DialogueState::AddAdmin(Step::Confirming { target }) = current else {
                ui::toast(&state.bot, q.id.clone(), "That step has moved on.").await;
                return Ok(());
            };
            apply(state, dialogue, surface, target).await
        }
        _ => {
            ui::toast(&state.bot, q.id.clone(), "That step has moved on.").await;
            Ok(())
        }
    }
}

async fn await_user_id(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    // A demotion between starting and finishing must stop the flow immediately.
    if reply::require_admin(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        super::reset(&dialogue).await;
        return Ok(());
    }

    let outcome = await_user_id_body(&state, &dialogue, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "add_admin.id", outcome).await
}

async fn await_user_id_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
) -> Result<()> {
    let target = text_of(msg)
        .and_then(|text| text.parse::<i64>().ok())
        .filter(|id| *id > 0);

    let Some(target) = target else {
        return super::reprompt(state, msg, copy::BAD_ADMIN_ID).await;
    };

    present_confirm(state, dialogue, Surface::New(msg.chat.id), target).await
}

async fn present_confirm(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    target: i64,
) -> Result<()> {
    let already_known = UserRepo::new(&state.db)
        .find_by_telegram_id(target)
        .await?
        .is_some();

    let rows = vec![
        vec![button("✅ Add admin", "ad:addok")],
        vec![button("✕ Cancel", CANCEL)],
    ];

    super::advance(dialogue, Step::Confirming { target }).await?;
    ui::render(
        &state.bot,
        surface,
        Screen::new(copy::confirm_admin(target, already_known), rows),
    )
    .await
}

async fn confirm(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    target: i64,
) -> HandlerResult {
    if reply::require_admin(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        super::reset(&dialogue).await;
        return Ok(());
    }

    let outcome = if is_affirmative(text_of(&msg)) {
        apply(&state, &dialogue, Surface::New(msg.chat.id), target).await
    } else {
        super::reset(&dialogue).await;
        super::reprompt(&state, &msg, "Cancelled — no admin was added.").await
    };
    reply::finish(&state.bot, msg.chat.id, "add_admin.confirm", outcome).await
}

async fn apply(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    target: i64,
) -> Result<()> {
    let user = UserRepo::new(&state.db).upsert(target, Role::Admin).await?;

    // The new admin's command menu gains its admin entries without a restart.
    menu::publish_for_admin(&state.bot, target, true).await;

    let note = if user.blocked {
        " They're still blocked — unblock them to restore access."
    } else {
        ""
    };

    let text = format!("✅ <code>{target}</code> is now an admin.{}", ui::esc(note));
    let rows = vec![vec![button("👥 Users", "ad:u"), button("🏠 Menu", MAIN)]];

    super::reset(dialogue).await;
    ui::render(&state.bot, surface, Screen::new(text, rows)).await
}
