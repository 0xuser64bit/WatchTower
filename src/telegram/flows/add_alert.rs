//! Guided alert rule creation.
//!
//! The target is chosen from the tracked tokens and wallets rather than typed as a
//! free-text address, keeping every rule attached to a relational target.

use crate::app_state::AppState;
use crate::db::repos::rules::{NewRuleTarget, RuleRepo};
use crate::db::repos::tokens::TokenRepo;
use crate::db::repos::wallets::WalletRepo;
use crate::error::Result;
use crate::rules::types::{Operator, TargetKind};
use crate::telegram::flows::{is_affirmative, reprompt, text_of, FlowDialogue, HandlerResult};
use crate::telegram::{copy, reply};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;

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

pub async fn start(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = start_body(&state, &dialogue, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "add_alert.start", outcome).await
}

async fn start_body(state: &AppState, dialogue: &FlowDialogue, msg: &Message) -> Result<()> {
    let tokens = TokenRepo::new(&state.db).count().await?;
    let wallets = WalletRepo::new(&state.db).count().await?;

    if tokens == 0 && wallets == 0 {
        reply::send_text(&state.bot, msg.chat.id, copy::NOTHING_TO_ALERT_ON).await?;
        return Ok(());
    }

    reply::send_text(
        &state.bot,
        msg.chat.id,
        copy::ask_alert_kind(tokens, wallets),
    )
    .await?;

    super::advance(dialogue, Step::AwaitingKind).await?;
    Ok(())
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

    let listing = match kind {
        TargetKind::Token => TokenRepo::new(&state.db)
            .list()
            .await?
            .into_iter()
            .map(|token| {
                format!(
                    "{}. {} — {}",
                    token.id,
                    token.symbol.as_deref().unwrap_or("no symbol"),
                    token.mint_address
                )
            })
            .collect::<Vec<_>>(),
        TargetKind::Wallet => WalletRepo::new(&state.db)
            .list()
            .await?
            .into_iter()
            .map(|wallet| {
                format!(
                    "{}. {} — {}",
                    wallet.id,
                    wallet.label.as_deref().unwrap_or("no label"),
                    wallet.address
                )
            })
            .collect::<Vec<_>>(),
    };

    if listing.is_empty() {
        reply::send_text(
            &state.bot,
            msg.chat.id,
            format!(
                "No {}s are tracked yet. Add one with /add{} first.",
                kind.as_str(),
                kind.as_str()
            ),
        )
        .await?;
        super::reset(dialogue).await;
        return Ok(());
    }

    reply::send_text(&state.bot, msg.chat.id, copy::ask_which_target(&listing)).await?;

    super::advance(dialogue, Step::AwaitingTarget { kind }).await?;
    Ok(())
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

    // Resolve now so a rule can never be created against a target that does not
    // exist; the foreign key would reject it later with an opaque error.
    let exists = match kind {
        TargetKind::Token => TokenRepo::new(&state.db).find(target_id).await?.is_some(),
        TargetKind::Wallet => WalletRepo::new(&state.db).find(target_id).await?.is_some(),
    };

    if !exists {
        return reprompt(
            state,
            msg,
            "No tracked target has that number. Send one from the list above.",
        )
        .await;
    }

    let (unit, subject, example_high, example_low) = match kind {
        TargetKind::Token => ("USD", "price", "250", "0.99"),
        TargetKind::Wallet => ("SOL", "balance", "100", "5"),
    };

    reply::send_text(
        &state.bot,
        msg.chat.id,
        copy::ask_operator(subject, unit, example_high, example_low),
    )
    .await?;

    super::advance(dialogue, Step::AwaitingOperator { kind, target_id }).await?;
    Ok(())
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

    reply::send_text(&state.bot, msg.chat.id, prompt).await?;

    super::advance(
        dialogue,
        Step::AwaitingThreshold {
            kind,
            target_id,
            operator,
        },
    )
    .await?;
    Ok(())
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

    let default = state.settings.alert_default_cooldown_seconds;
    reply::send_text(&state.bot, msg.chat.id, copy::ask_cooldown(default)).await?;

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
    Ok(())
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

    let summary = describe(
        state,
        kind,
        target_id,
        operator,
        threshold,
        cooldown_seconds,
    )
    .await?;

    reply::send_text(&state.bot, msg.chat.id, summary).await?;

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
    Ok(())
}

async fn confirm(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (kind, target_id, operator, threshold, cooldown_seconds): (TargetKind, i64, Operator, f64, i64),
) -> HandlerResult {
    let outcome = confirm_body(
        &state,
        &dialogue,
        &msg,
        (kind, target_id, operator, threshold, cooldown_seconds),
    )
    .await;
    reply::finish(&state.bot, msg.chat.id, "add_alert.confirm", outcome).await
}

async fn confirm_body(
    state: &AppState,
    dialogue: &FlowDialogue,
    msg: &Message,
    (kind, target_id, operator, threshold, cooldown_seconds): (TargetKind, i64, Operator, f64, i64),
) -> Result<()> {
    if !is_affirmative(text_of(msg)) {
        super::reset(dialogue).await;
        return reprompt(state, msg, copy::CANCELLED_NO_ALERT).await;
    }

    let target = match kind {
        TargetKind::Token => NewRuleTarget::Token { id: target_id },
        TargetKind::Wallet => NewRuleTarget::Wallet { id: target_id },
    };

    match RuleRepo::new(&state.db)
        .create(target, operator, threshold, cooldown_seconds)
        .await
    {
        Ok(rule) => {
            reply::send_text(
                &state.bot,
                msg.chat.id,
                copy::alert_saved(
                    rule.id,
                    &rule.target.display(),
                    &rule.condition(),
                    state.settings.poll_interval.as_secs(),
                ),
            )
            .await?;
        }
        Err(err) => reply::report_error(&state.bot, msg.chat.id, "add_alert", &err).await,
    }

    super::reset(dialogue).await;
    Ok(())
}

async fn describe(
    state: &AppState,
    kind: TargetKind,
    target_id: i64,
    operator: Operator,
    threshold: f64,
    cooldown_seconds: i64,
) -> crate::error::Result<String> {
    let target = match kind {
        TargetKind::Token => TokenRepo::new(&state.db)
            .find(target_id)
            .await?
            .map(|token| token.display()),
        TargetKind::Wallet => WalletRepo::new(&state.db)
            .find(target_id)
            .await?
            .map(|wallet| wallet.display()),
    }
    .unwrap_or_else(|| format!("{} {target_id}", kind.as_str()));

    let condition = if operator.is_percentage() {
        format!(
            "{} {} {}%",
            kind.metric(),
            operator.symbol(),
            crate::alerts::format::amount(threshold)
        )
    } else {
        format!(
            "{} {} {} {}",
            kind.metric(),
            operator.symbol(),
            crate::alerts::format::amount(threshold),
            kind.unit()
        )
    };

    Ok(copy::confirm_alert(&target, &condition, cooldown_seconds))
}
