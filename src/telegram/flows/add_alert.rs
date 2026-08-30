//! Guided alert creation.
//!
//! The interaction is keyboard-first: kind, target and condition are all tapped, and
//! only the threshold (and, optionally, the cooldown) are typed. Every discrete step
//! is also reachable by typing the equivalent word, so a power user — and the test
//! suite — can drive the whole flow from the keyboard if they prefer.
//!
//! Each step is presented by a `present_*` helper that renders onto a [`Surface`] and
//! advances the dialogue. Text handlers call them with a fresh message; callback
//! handlers edit the tapped message in place. The target is always chosen from tracked
//! tokens and wallets, so a rule can never point at an untracked address.

use crate::alerts::format;
use crate::app_state::AppState;
use crate::db::repos::rules::{NewRuleTarget, RuleRepo};
use crate::db::repos::tokens::TokenRepo;
use crate::db::repos::wallets::WalletRepo;
use crate::error::Result;
use crate::rules::types::{Operator, TargetKind};
use crate::telegram::callback::CANCEL;
use crate::telegram::flows::{
    is_affirmative, reprompt, text_of, DialogueState, FlowDialogue, HandlerResult,
};
use crate::telegram::ui::{self, button, esc, Screen, Surface};
use crate::telegram::{copy, reply};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, InlineKeyboardButton};

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    AwaitingKind,
    AwaitingTarget {
        kind: TargetKind,
    },
    AwaitingOperator {
        kind: TargetKind,
        target_id: i64,
    },
    AwaitingThreshold {
        kind: TargetKind,
        target_id: i64,
        operator: Operator,
    },
    AwaitingCooldown {
        kind: TargetKind,
        target_id: i64,
        operator: Operator,
        threshold: f64,
    },
    Confirming {
        kind: TargetKind,
        target_id: i64,
        operator: Operator,
        threshold: f64,
        cooldown_seconds: i64,
    },
}

pub fn handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    dptree::entry()
        .branch(case![Step::AwaitingKind].endpoint(await_kind))
        .branch(case![Step::AwaitingTarget { kind }].endpoint(await_target))
        .branch(case![Step::AwaitingOperator { kind, target_id }].endpoint(await_operator))
        .branch(
            case![Step::AwaitingThreshold {
                kind,
                target_id,
                operator
            }]
            .endpoint(await_threshold),
        )
        .branch(
            case![Step::AwaitingCooldown {
                kind,
                target_id,
                operator,
                threshold
            }]
            .endpoint(await_cooldown),
        )
        .branch(
            case![Step::Confirming {
                kind,
                target_id,
                operator,
                threshold,
                cooldown_seconds
            }]
            .endpoint(confirm),
        )
}

// ── Entry points ──────────────────────────────────────────────────────────────────

/// `/addalert` command entry.
pub async fn start(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = start_on(&state, &dialogue, Surface::New(msg.chat.id)).await;
    reply::finish(&state.bot, msg.chat.id, "add_alert.start", outcome).await
}

/// Shared entry used by the command and the "Create Alert" button. Assumes the caller
/// is already authorized.
pub async fn start_on(state: &AppState, dialogue: &FlowDialogue, surface: Surface) -> Result<()> {
    present_kind(state, dialogue, surface).await
}

/// Routes an `ac:*` in-flow tap. Reads accumulated choices from `current`; a tap whose
/// step no longer matches (a stale keyboard, a double tap) is answered with a notice
/// rather than acted on.
pub async fn on_callback(
    state: &AppState,
    dialogue: &FlowDialogue,
    current: DialogueState,
    surface: Surface,
    q: &CallbackQuery,
    rest: &[&str],
) -> Result<()> {
    match rest {
        ["k", code] => {
            let DialogueState::AddAlert(Step::AwaitingKind) = current else {
                return expired(state, q).await;
            };
            let kind = match *code {
                "t" => TargetKind::Token,
                "w" => TargetKind::Wallet,
                _ => return expired(state, q).await,
            };
            present_target(state, dialogue, surface, kind).await
        }
        ["tg", id] => {
            let DialogueState::AddAlert(Step::AwaitingTarget { kind }) = current else {
                return expired(state, q).await;
            };
            let Ok(target_id) = id.parse::<i64>() else {
                return expired(state, q).await;
            };
            present_operator(state, dialogue, surface, kind, target_id).await
        }
        ["op", code] => {
            let DialogueState::AddAlert(Step::AwaitingOperator { kind, target_id }) = current
            else {
                return expired(state, q).await;
            };
            let Some(operator) = op_from_code(code) else {
                return expired(state, q).await;
            };
            present_threshold(state, dialogue, surface, kind, target_id, operator).await
        }
        ["cd"] => {
            let DialogueState::AddAlert(Step::AwaitingCooldown {
                kind,
                target_id,
                operator,
                threshold,
            }) = current
            else {
                return expired(state, q).await;
            };
            let cooldown = state.settings.alert_default_cooldown_seconds;
            present_confirm(
                state, dialogue, surface, kind, target_id, operator, threshold, cooldown,
            )
            .await
        }
        ["ok"] => {
            let DialogueState::AddAlert(Step::Confirming {
                kind,
                target_id,
                operator,
                threshold,
                cooldown_seconds,
            }) = current
            else {
                return expired(state, q).await;
            };
            create(
                state,
                dialogue,
                surface,
                kind,
                target_id,
                operator,
                threshold,
                cooldown_seconds,
            )
            .await
        }
        ["bk"] => back(state, dialogue, current, surface, q).await,
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

/// Steps one screen back, re-rendering in place.
async fn back(
    state: &AppState,
    dialogue: &FlowDialogue,
    current: DialogueState,
    surface: Surface,
    q: &CallbackQuery,
) -> Result<()> {
    match current {
        DialogueState::AddAlert(Step::AwaitingTarget { .. }) => {
            present_kind(state, dialogue, surface).await
        }
        DialogueState::AddAlert(Step::AwaitingOperator { kind, .. }) => {
            present_target(state, dialogue, surface, kind).await
        }
        DialogueState::AddAlert(Step::AwaitingThreshold {
            kind, target_id, ..
        }) => present_operator(state, dialogue, surface, kind, target_id).await,
        DialogueState::AddAlert(Step::AwaitingCooldown {
            kind,
            target_id,
            operator,
            ..
        }) => present_threshold(state, dialogue, surface, kind, target_id, operator).await,
        DialogueState::AddAlert(Step::Confirming {
            kind,
            target_id,
            operator,
            threshold,
            ..
        }) => {
            present_cooldown(
                state, dialogue, surface, kind, target_id, operator, threshold,
            )
            .await
        }
        _ => expired(state, q).await,
    }
}

// ── Step 1: kind ────────────────────────────────────────────────────────────────────

async fn present_kind(state: &AppState, dialogue: &FlowDialogue, surface: Surface) -> Result<()> {
    let tokens = TokenRepo::new(&state.db).count().await?;
    let wallets = WalletRepo::new(&state.db).count().await?;

    if tokens == 0 && wallets == 0 {
        let rows = vec![
            vec![
                button("⭐ Popular Tokens", "at:pop"),
                button("👛 Add Wallet", "aw:new"),
            ],
            crate::telegram::ui::menu_row(),
        ];
        super::reset(dialogue).await;
        return ui::render(
            &state.bot,
            surface,
            Screen::new(copy::NOTHING_TO_ALERT_ON, rows),
        )
        .await;
    }

    let rows = vec![
        vec![
            button(format!("🪙 Token ({tokens})"), "ac:k:t"),
            button(format!("👛 Wallet ({wallets})"), "ac:k:w"),
        ],
        vec![button("✕ Cancel", CANCEL)],
    ];

    super::advance(dialogue, Step::AwaitingKind).await?;
    ui::render(&state.bot, surface, Screen::new(copy::ASK_ALERT_KIND, rows)).await
}

async fn await_kind(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let outcome = await_kind_body(&state, &dialogue, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "add_alert.kind", outcome).await
}

async fn await_kind_body(state: &AppState, dialogue: &FlowDialogue, msg: &Message) -> Result<()> {
    let kind = match text_of(msg).map(|text| text.to_ascii_lowercase()) {
        Some(text) if text == "token" => TargetKind::Token,
        Some(text) if text == "wallet" => TargetKind::Wallet,
        _ => return reprompt(state, msg, copy::BAD_ALERT_KIND).await,
    };

    present_target(state, dialogue, Surface::New(msg.chat.id), kind).await
}

// ── Step 2: target ──────────────────────────────────────────────────────────────────

async fn present_target(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    kind: TargetKind,
) -> Result<()> {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    match kind {
        TargetKind::Token => {
            for token in TokenRepo::new(&state.db).list().await? {
                let name = token
                    .symbol
                    .clone()
                    .unwrap_or_else(|| "unnamed".to_string());
                rows.push(vec![button(
                    format!(
                        "🪙 {} · {}",
                        name,
                        crate::rules::types::abbreviate(&token.mint_address)
                    ),
                    format!("ac:tg:{}", token.id),
                )]);
            }
        }
        TargetKind::Wallet => {
            for wallet in WalletRepo::new(&state.db).list().await? {
                let name = wallet
                    .label
                    .clone()
                    .unwrap_or_else(|| "unnamed".to_string());
                rows.push(vec![button(
                    format!(
                        "👛 {} · {}",
                        name,
                        crate::rules::types::abbreviate(&wallet.address)
                    ),
                    format!("ac:tg:{}", wallet.id),
                )]);
            }
        }
    }

    if rows.is_empty() {
        // The user picked a kind that has no tracked targets yet.
        let add = match kind {
            TargetKind::Token => vec![button("⭐ Popular Tokens", "at:pop")],
            TargetKind::Wallet => vec![button("👛 Add Wallet", "aw:new")],
        };
        super::reset(dialogue).await;
        return ui::render(
            &state.bot,
            surface,
            Screen::new(
                format!("Nothing to pick yet — add a {} first.", kind.as_str()),
                vec![add, crate::telegram::ui::menu_row()],
            ),
        )
        .await;
    }

    rows.push(vec![button("← Back", "ac:bk"), button("✕ Cancel", CANCEL)]);

    super::advance(dialogue, Step::AwaitingTarget { kind }).await?;
    ui::render(&state.bot, surface, Screen::new(copy::ASK_TARGET, rows)).await
}

async fn await_target(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    kind: TargetKind,
) -> HandlerResult {
    let outcome = await_target_body(&state, &dialogue, &msg, kind).await;
    reply::finish(&state.bot, msg.chat.id, "add_alert.target", outcome).await
}

async fn await_target_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
    kind: TargetKind,
) -> Result<()> {
    let Some(target_id) = text_of(msg).and_then(|text| text.parse::<i64>().ok()) else {
        return reprompt(state, msg, copy::BAD_TARGET_NUMBER).await;
    };

    if !target_exists(state, kind, target_id).await? {
        return reprompt(state, msg, copy::BAD_TARGET_NUMBER).await;
    }

    present_operator(state, dialogue, Surface::New(msg.chat.id), kind, target_id).await
}

async fn target_exists(state: &AppState, kind: TargetKind, target_id: i64) -> Result<bool> {
    Ok(match kind {
        TargetKind::Token => TokenRepo::new(&state.db).find(target_id).await?.is_some(),
        TargetKind::Wallet => WalletRepo::new(&state.db).find(target_id).await?.is_some(),
    })
}

// ── Step 3: operator ────────────────────────────────────────────────────────────────

async fn present_operator(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    kind: TargetKind,
    target_id: i64,
) -> Result<()> {
    // Resolve now so a rule can never be created against a target that vanished.
    if !target_exists(state, kind, target_id).await? {
        super::reset(dialogue).await;
        return ui::render(
            &state.bot,
            surface,
            Screen::new(
                "That target is no longer tracked.",
                vec![crate::telegram::ui::menu_row()],
            ),
        )
        .await;
    }

    let rows = vec![
        vec![
            button("⬆️ Above", "ac:op:gt"),
            button("⬇️ Below", "ac:op:lt"),
        ],
        vec![
            button("≥ At or above", "ac:op:gte"),
            button("≤ At or below", "ac:op:lte"),
        ],
        vec![
            button("📈 Up %", "ac:op:up"),
            button("📉 Down %", "ac:op:dn"),
        ],
        vec![button("← Back", "ac:bk"), button("✕ Cancel", CANCEL)],
    ];

    super::advance(dialogue, Step::AwaitingOperator { kind, target_id }).await?;
    ui::render(&state.bot, surface, Screen::new(copy::ASK_OPERATOR, rows)).await
}

async fn await_operator(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (kind, target_id): (TargetKind, i64),
) -> HandlerResult {
    let outcome = await_operator_body(&state, &dialogue, &msg, (kind, target_id)).await;
    reply::finish(&state.bot, msg.chat.id, "add_alert.operator", outcome).await
}

async fn await_operator_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
    (kind, target_id): (TargetKind, i64),
) -> Result<()> {
    let Some(operator) = text_of(msg).and_then(Operator::parse) else {
        return reprompt(state, msg, copy::BAD_OPERATOR).await;
    };

    present_threshold(
        state,
        dialogue,
        Surface::New(msg.chat.id),
        kind,
        target_id,
        operator,
    )
    .await
}

// ── Step 4: threshold ───────────────────────────────────────────────────────────────

async fn present_threshold(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    kind: TargetKind,
    target_id: i64,
    operator: Operator,
) -> Result<()> {
    let prompt = if operator.is_percentage() {
        copy::ASK_PERCENT.to_string()
    } else {
        copy::ask_threshold(
            match kind {
                TargetKind::Token => "1.5",
                TargetKind::Wallet => "5",
            },
            kind.unit(),
        )
    };

    let rows = vec![vec![button("← Back", "ac:bk"), button("✕ Cancel", CANCEL)]];

    super::advance(
        dialogue,
        Step::AwaitingThreshold {
            kind,
            target_id,
            operator,
        },
    )
    .await?;
    ui::render(&state.bot, surface, Screen::new(prompt, rows)).await
}

async fn await_threshold(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (kind, target_id, operator): (TargetKind, i64, Operator),
) -> HandlerResult {
    let outcome = await_threshold_body(&state, &dialogue, &msg, (kind, target_id, operator)).await;
    reply::finish(&state.bot, msg.chat.id, "add_alert.threshold", outcome).await
}

async fn await_threshold_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
    (kind, target_id, operator): (TargetKind, i64, Operator),
) -> Result<()> {
    let threshold = text_of(msg)
        .map(|text| text.trim_start_matches('+').replace(['%', ',', '_'], ""))
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0);

    let Some(threshold) = threshold else {
        return reprompt(state, msg, copy::BAD_THRESHOLD).await;
    };

    if operator.is_percentage() && threshold > 1000.0 {
        return reprompt(state, msg, copy::THRESHOLD_TOO_BIG).await;
    }

    present_cooldown(
        state,
        dialogue,
        Surface::New(msg.chat.id),
        kind,
        target_id,
        operator,
        threshold,
    )
    .await
}

// ── Step 5: cooldown ────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn present_cooldown(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    kind: TargetKind,
    target_id: i64,
    operator: Operator,
    threshold: f64,
) -> Result<()> {
    let default = state.settings.alert_default_cooldown_seconds;

    let rows = vec![
        vec![button(format!("Use default ({default}s)"), "ac:cd")],
        vec![button("← Back", "ac:bk"), button("✕ Cancel", CANCEL)],
    ];

    super::advance(
        dialogue,
        Step::AwaitingCooldown {
            kind,
            target_id,
            operator,
            threshold,
        },
    )
    .await?;
    ui::render(
        &state.bot,
        surface,
        Screen::new(copy::ask_cooldown(default), rows),
    )
    .await
}

async fn await_cooldown(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (kind, target_id, operator, threshold): (TargetKind, i64, Operator, f64),
) -> HandlerResult {
    let outcome = await_cooldown_body(
        &state,
        &dialogue,
        &msg,
        (kind, target_id, operator, threshold),
    )
    .await;
    reply::finish(&state.bot, msg.chat.id, "add_alert.cooldown", outcome).await
}

async fn await_cooldown_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
    (kind, target_id, operator, threshold): (TargetKind, i64, Operator, f64),
) -> Result<()> {
    let Some(raw) = text_of(msg) else {
        return reprompt(state, msg, copy::BAD_COOLDOWN).await;
    };

    // `skip`, `none` and `-` all mean "use the configured default".
    let cooldown_seconds = match super::optional_answer(raw) {
        None => state.settings.alert_default_cooldown_seconds,
        Some(answer) => match answer.parse::<i64>() {
            Ok(value) if (0..=86_400).contains(&value) => value,
            _ => return reprompt(state, msg, copy::BAD_COOLDOWN).await,
        },
    };

    present_confirm(
        state,
        dialogue,
        Surface::New(msg.chat.id),
        kind,
        target_id,
        operator,
        threshold,
        cooldown_seconds,
    )
    .await
}

// ── Step 6: confirm ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn present_confirm(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    kind: TargetKind,
    target_id: i64,
    operator: Operator,
    threshold: f64,
    cooldown_seconds: i64,
) -> Result<()> {
    let target = target_line(state, kind, target_id).await?;
    let condition = format::condition(kind, operator, threshold);
    let summary = copy::confirm_alert(&target, &condition, cooldown_seconds);

    let rows = vec![
        vec![button("✅ Create Alert", "ac:ok")],
        vec![button("✎ Edit", "ac:bk"), button("✕ Cancel", CANCEL)],
    ];

    super::advance(
        dialogue,
        Step::Confirming {
            kind,
            target_id,
            operator,
            threshold,
            cooldown_seconds,
        },
    )
    .await?;
    ui::render(&state.bot, surface, Screen::new(summary, rows)).await
}

async fn confirm(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (kind, target_id, operator, threshold, cooldown_seconds): (TargetKind, i64, Operator, f64, i64),
) -> HandlerResult {
    let outcome = if is_affirmative(text_of(&msg)) {
        create(
            &state,
            &dialogue,
            Surface::New(msg.chat.id),
            kind,
            target_id,
            operator,
            threshold,
            cooldown_seconds,
        )
        .await
    } else {
        super::reset(&dialogue).await;
        reprompt(&state, &msg, copy::CANCELLED_NO_ALERT).await
    };
    reply::finish(&state.bot, msg.chat.id, "add_alert.confirm", outcome).await
}

#[allow(clippy::too_many_arguments)]
async fn create(
    state: &AppState,
    dialogue: &FlowDialogue,
    surface: Surface,
    kind: TargetKind,
    target_id: i64,
    operator: Operator,
    threshold: f64,
    cooldown_seconds: i64,
) -> Result<()> {
    let target = match kind {
        TargetKind::Token => NewRuleTarget::Token { id: target_id },
        TargetKind::Wallet => NewRuleTarget::Wallet { id: target_id },
    };

    match RuleRepo::new(&state.db)
        .create(target, operator, threshold, cooldown_seconds)
        .await
    {
        Ok(rule) => {
            let line = target_line(state, kind, target_id)
                .await
                .unwrap_or_else(|_| esc(&rule.target.name()));
            let condition = format::condition(kind, operator, threshold);
            let text = copy::alert_saved(&line, &condition, state.settings.poll_interval.as_secs());
            let rows = vec![vec![
                button("🚨 View Alerts", "al"),
                button("🏠 Menu", crate::telegram::callback::MAIN),
            ]];
            super::reset(dialogue).await;
            ui::render(&state.bot, surface, Screen::new(text, rows)).await?;
        }
        Err(err) => {
            super::reset(dialogue).await;
            reply::report_error(&state.bot, surface.chat(), "add_alert", &err).await;
        }
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────────────

async fn target_line(state: &AppState, kind: TargetKind, target_id: i64) -> Result<String> {
    let (icon, name) = match kind {
        TargetKind::Token => {
            let token = TokenRepo::new(&state.db).find(target_id).await?;
            let name = token
                .and_then(|t| {
                    t.symbol
                        .clone()
                        .or_else(|| Some(crate::rules::types::abbreviate(&t.mint_address)))
                })
                .unwrap_or_else(|| "token".to_string());
            ("🪙", name)
        }
        TargetKind::Wallet => {
            let wallet = WalletRepo::new(&state.db).find(target_id).await?;
            let name = wallet
                .and_then(|w| {
                    w.label
                        .clone()
                        .or_else(|| Some(crate::rules::types::abbreviate(&w.address)))
                })
                .unwrap_or_else(|| "wallet".to_string());
            ("👛", name)
        }
    };

    Ok(format!("{icon} <b>{}</b>", esc(&name)))
}

fn op_from_code(code: &str) -> Option<Operator> {
    match code {
        "gt" => Some(Operator::Gt),
        "lt" => Some(Operator::Lt),
        "gte" => Some(Operator::Gte),
        "lte" => Some(Operator::Lte),
        "up" => Some(Operator::PctUp),
        "dn" => Some(Operator::PctDown),
        _ => None,
    }
}
