//! Guided alert rule creation.
//!
//! The target is chosen from the tracked tokens and wallets rather than typed as a
//! free-text address. Previously any string that looked like base58 was accepted and
//! stored on the rule with no relation to the tracked directory, so it was possible
//! to create an alert for something the user had never added — and deleting a token
//! left its rules polling forever.

use crate::app_state::AppState;
use crate::db::repos::rules::{NewRuleTarget, RuleRepo};
use crate::db::repos::tokens::TokenRepo;
use crate::db::repos::wallets::WalletRepo;
use crate::rules::types::{Operator, TargetKind};
use crate::telegram::flows::{is_affirmative, reprompt, text_of, FlowDialogue, HandlerResult};
use crate::telegram::reply;
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

    let tokens = TokenRepo::new(&state.db).count().await?;
    let wallets = WalletRepo::new(&state.db).count().await?;

    if tokens == 0 && wallets == 0 {
        reply::send_text(
            &state.bot,
            msg.chat.id,
            "Nothing is tracked yet. Add a target first with /addtoken or /addwallet.",
        )
        .await?;
        return Ok(());
    }

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "What should this alert watch?\n\
             Send `token` for a token price ({tokens} tracked) or `wallet` for a SOL balance \
             ({wallets} tracked).\n\nSend /cancel to stop."
        ),
    )
    .await?;

    dialogue.update(Step::AwaitingKind).await?;
    Ok(())
}

async fn await_kind(state: AppState, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let kind = match text_of(&msg).map(|text| text.to_ascii_lowercase()) {
        Some(text) if text == "token" => TargetKind::Token,
        Some(text) if text == "wallet" => TargetKind::Wallet,
        _ => return reprompt(&state, &msg, "Send `token` or `wallet`.").await,
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
        super::reset(&dialogue).await;
        return Ok(());
    }

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("Which one? Send its number.\n\n{}", listing.join("\n")),
    )
    .await?;

    dialogue.update(Step::AwaitingTarget { kind }).await?;
    Ok(())
}

async fn await_target(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    kind: TargetKind,
) -> HandlerResult {
    let Some(target_id) = text_of(&msg).and_then(|text| text.parse::<i64>().ok()) else {
        return reprompt(&state, &msg, "Send the number shown next to the target.").await;
    };

    // Resolve now so a rule can never be created against a target that does not
    // exist; the foreign key would reject it later with an opaque error.
    let exists = match kind {
        TargetKind::Token => TokenRepo::new(&state.db).find(target_id).await?.is_some(),
        TargetKind::Wallet => WalletRepo::new(&state.db).find(target_id).await?.is_some(),
    };

    if !exists {
        return reprompt(
            &state,
            &msg,
            "No tracked target has that number. Send one from the list above.",
        )
        .await;
    }

    let unit = kind.unit();
    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "How should it be compared?\n\n\
             `>`  above a {unit} value\n\
             `<`  below a {unit} value\n\
             `>=` at or above a {unit} value\n\
             `<=` at or below a {unit} value\n\
             `%up`   rose by a percentage\n\
             `%down` fell by a percentage"
        ),
    )
    .await?;

    dialogue
        .update(Step::AwaitingOperator { kind, target_id })
        .await?;
    Ok(())
}

async fn await_operator(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (kind, target_id): (TargetKind, i64),
) -> HandlerResult {
    let Some(operator) = text_of(&msg).and_then(Operator::parse) else {
        return reprompt(&state, &msg, "Send one of: >, <, >=, <=, %up, %down.").await;
    };

    let prompt = if operator.is_percentage() {
        "Send the percentage change that should trigger the alert, for example `10` for 10%.\n\n\
         The baseline is the first value observed after the rule is created, and it re-baselines \
         each time the alert fires."
            .to_string()
    } else {
        format!(
            "Send the {} threshold as a number, for example `1.5`.",
            kind.unit()
        )
    };

    reply::send_text(&state.bot, msg.chat.id, prompt).await?;

    dialogue
        .update(Step::AwaitingThreshold {
            kind,
            target_id,
            operator,
        })
        .await?;
    Ok(())
}

async fn await_threshold(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (kind, target_id, operator): (TargetKind, i64, Operator),
) -> HandlerResult {
    let threshold = text_of(&msg)
        .map(|text| text.trim_start_matches('+').replace(['%', ',', '_'], ""))
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0);

    let Some(threshold) = threshold else {
        return reprompt(
            &state,
            &msg,
            "Send a positive number. Use `%down 10` style percentages as just `10`.",
        )
        .await;
    };

    if operator.is_percentage() && threshold > 1000.0 {
        return reprompt(
            &state,
            &msg,
            "Percentage thresholds above 1000% are rejected as typos.",
        )
        .await;
    }

    let default = state.settings.alert_default_cooldown_seconds;
    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "Minimum seconds between repeat alerts for this rule?\n\n\
             Send a number, or `-` to use the configured default of {default}s.\n\n\
             Note: an alert fires when the condition becomes true and then stays quiet \
             until the condition clears, so this only limits a condition that keeps \
             flipping back and forth."
        ),
    )
    .await?;

    dialogue
        .update(Step::AwaitingCooldown {
            kind,
            target_id,
            operator,
            threshold,
        })
        .await?;
    Ok(())
}

async fn await_cooldown(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (kind, target_id, operator, threshold): (TargetKind, i64, Operator, f64),
) -> HandlerResult {
    let Some(raw) = text_of(&msg) else {
        return reprompt(
            &state,
            &msg,
            "Send a number of seconds, or `-` for the default.",
        )
        .await;
    };

    let cooldown_seconds =
        if raw == "-" {
            state.settings.alert_default_cooldown_seconds
        } else {
            match raw.parse::<i64>() {
                Ok(value) if (0..=86_400).contains(&value) => value,
                _ => return reprompt(
                    &state,
                    &msg,
                    "Send a whole number of seconds between 0 and 86400, or `-` for the default.",
                )
                .await,
            }
        };

    let summary = describe(
        &state,
        kind,
        target_id,
        operator,
        threshold,
        cooldown_seconds,
    )
    .await?;

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("Create this alert?\n\n{summary}\n\nReply `yes` to confirm, or /cancel."),
    )
    .await?;

    dialogue
        .update(Step::Confirming {
            kind,
            target_id,
            operator,
            threshold,
            cooldown_seconds,
        })
        .await?;
    Ok(())
}

async fn confirm(
    state: AppState,
    dialogue: FlowDialogue,
    msg: Message,
    (kind, target_id, operator, threshold, cooldown_seconds): (TargetKind, i64, Operator, f64, i64),
) -> HandlerResult {
    if !is_affirmative(text_of(&msg)) {
        super::reset(&dialogue).await;
        return reprompt(&state, &msg, "Cancelled. No alert was created.").await;
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
                format!(
                    "Alert {} created: {} {}.\n\nIt is active now. Use /alerts to review.",
                    rule.id,
                    rule.target.display(),
                    rule.condition()
                ),
            )
            .await?;
        }
        Err(err) => reply::report_error(&state.bot, msg.chat.id, "add_alert", &err).await,
    }

    super::reset(&dialogue).await;
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

    Ok(format!(
        "Target: {target}\nCondition: {condition}\nCooldown: {cooldown_seconds}s"
    ))
}
