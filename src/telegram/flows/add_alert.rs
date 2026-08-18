use crate::db::repos::rules::RuleRepo;
use crate::db::Db;
use std::sync::Arc;
use teloxide::dispatching::dialogue::{Dialogue, InMemStorage};
use teloxide::dispatching::UpdateHandler;
use teloxide::prelude::*;

pub type FlowDialogue = Dialogue<AddAlertState, InMemStorage<AddAlertState>>;
type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Default)]
pub enum AddAlertState {
    #[default]
    AwaitingKind,
    AwaitingTarget { kind: String },
    AwaitingOperator { kind: String, target: String },
    AwaitingThreshold { kind: String, target: String, operator: String },
    Confirm { kind: String, target: String, operator: String, threshold: f64 },
}

pub fn message_handler() -> UpdateHandler<Box<dyn std::error::Error + Send + Sync>> {
    use dptree::case;

    Update::filter_message()
        .branch(case![AddAlertState::AwaitingKind].endpoint(await_kind))
        .branch(case![AddAlertState::AwaitingTarget { kind }].endpoint(await_target))
        .branch(case![AddAlertState::AwaitingOperator { kind, target }].endpoint(await_operator))
        .branch(case![AddAlertState::AwaitingThreshold { kind, target, operator }].endpoint(await_threshold))
        .branch(case![AddAlertState::Confirm { kind, target, operator, threshold }].endpoint(confirm))
}

async fn await_kind(bot: Bot, dialogue: FlowDialogue, msg: Message) -> HandlerResult {
    let Some(text) = msg.text().map(|s| s.trim().to_lowercase()) else {
        bot.send_message(msg.chat.id, "Send the alert kind: `price` or `balance`.").await?;
        return Ok(());
    };

    if text != "price" && text != "balance" {
        bot.send_message(msg.chat.id, "Kind must be `price` or `balance`.").await?;
        return Ok(());
    }

    let prompt = if text == "price" {
        "Send the token mint address to track."
    } else {
        "Send the wallet address to monitor."
    };

    bot.send_message(msg.chat.id, prompt).await?;
    dialogue.update(AddAlertState::AwaitingTarget { kind: text }).await?;
    Ok(())
}

async fn await_target(
    bot: Bot,
    dialogue: FlowDialogue,
    msg: Message,
    kind: String,
) -> HandlerResult {
    let Some(target) = msg.text().map(|s| s.trim().to_string()) else {
        bot.send_message(msg.chat.id, "Send a valid Solana address.").await?;
        return Ok(());
    };

    if !crate::providers::solana::validation::is_valid_base58_address(&target) {
        bot.send_message(msg.chat.id, "Send a valid Solana address.").await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, "Send the operator: `>`, `<`, `>=`, `<=`, `%up`, or `%down`.").await?;
    dialogue.update(AddAlertState::AwaitingOperator { kind, target }).await?;
    Ok(())
}

async fn await_operator(
    bot: Bot,
    dialogue: FlowDialogue,
    msg: Message,
    kind: String,
    target: String,
) -> HandlerResult {
    let Some(operator) = msg.text().map(|s| s.trim().to_lowercase()) else {
        bot.send_message(msg.chat.id, "Send a valid operator.").await?;
        return Ok(());
    };

    let valid = matches!(operator.as_str(), ">" | "<" | ">=" | "<=" | "%up" | "%down");
    if !valid {
        bot.send_message(msg.chat.id, "Invalid operator.").await?;
        return Ok(());
    }

    bot.send_message(msg.chat.id, "Send the numeric threshold.").await?;
    dialogue.update(AddAlertState::AwaitingThreshold { kind, target, operator }).await?;
    Ok(())
}

async fn await_threshold(
    bot: Bot,
    dialogue: FlowDialogue,
    msg: Message,
    kind: String,
    target: String,
    operator: String,
) -> HandlerResult {
    let Some(threshold) = msg.text().and_then(|s| s.trim().parse::<f64>().ok()) else {
        bot.send_message(msg.chat.id, "Send a positive numeric threshold.").await?;
        return Ok(());
    };

    if !threshold.is_finite() || threshold <= 0.0 {
        bot.send_message(msg.chat.id, "Send a positive numeric threshold.").await?;
        return Ok(());
    }

    bot.send_message(
        msg.chat.id,
        format!("Create alert?\nKind: {kind}\nTarget: {target}\nOperator: {operator}\nThreshold: {threshold}\n\nReply `confirm` to create or `cancel` to abort."),
    )
    .await?;

    dialogue
        .update(AddAlertState::Confirm { kind, target, operator, threshold })
        .await?;
    Ok(())
}

async fn confirm(
    bot: Bot,
    dialogue: FlowDialogue,
    db: Arc<Db>,
    msg: Message,
    kind: String,
    target: String,
    operator: String,
    threshold: f64,
) -> HandlerResult {
    let reply = msg.text().map(|s| s.trim().to_lowercase());

    if reply != Some("confirm".to_string()) {
        bot.send_message(msg.chat.id, "Cancelled.").await?;
        dialogue.exit().await?;
        return Ok(());
    }

    let target_type = if kind == "price" { "token" } else { "wallet" };
    let metric = if kind == "price" { "price" } else { "balance" };

    match RuleRepo::new(&db)
        .create(
            &kind,
            target_type,
            &target,
            metric,
            &operator,
            threshold,
            None,
            300,
            None,
            None,
        )
        .await
    {
        Ok(_) => {
            bot.send_message(msg.chat.id, "Alert rule created.").await?;
        }
        Err(_) => {
            bot.send_message(msg.chat.id, "Failed to create alert rule.").await?;
        }
    }

    dialogue.exit().await?;
    Ok(())
}
