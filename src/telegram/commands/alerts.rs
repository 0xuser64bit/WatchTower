use crate::db::repos::alert_events::AlertEventRepo;
use crate::db::repos::rules::RuleRepo;
use crate::db::Db;
use crate::telegram::auth;
use std::sync::Arc;
use teloxide::prelude::*;

pub async fn list_alerts(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if auth::authorize_or_send(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let rules = RuleRepo::new(&db).list_all().await?;

    if rules.is_empty() {
        bot.send_message(
            msg.chat.id,
            "No alert rules yet. Use /addalert to create one.",
        )
        .await?;
        return Ok(());
    }

    let text = rules
        .iter()
        .map(|rule| {
            let status = if rule.is_enabled() {
                "enabled"
            } else {
                "disabled"
            };
            format!(
                "{}: {} {} {} {} on {} ({status})",
                rule.id, rule.kind, rule.metric, rule.operator, rule.threshold, rule.target_ref
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    bot.send_message(msg.chat.id, format!("Alert rules:\n{text}"))
        .await?;
    Ok(())
}

pub async fn show_history(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if auth::authorize_or_send(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let events = AlertEventRepo::new(&db).list_recent(10).await?;

    if events.is_empty() {
        bot.send_message(msg.chat.id, "No alerts fired yet.")
            .await?;
        return Ok(());
    }

    let text = events
        .iter()
        .map(|event| {
            format!(
                "#{}: {} ({} at {})",
                event.id,
                event.message,
                event.current_value,
                event.triggered_at.format("%Y-%m-%d %H:%M:%S UTC")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    bot.send_message(msg.chat.id, format!("Recent alerts:\n{text}"))
        .await?;
    Ok(())
}

pub async fn delete_rule(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    args: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if auth::authorize_or_send(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    let id = match args.trim().parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => {
            bot.send_message(msg.chat.id, "Usage: /deleterule <id>")
                .await?;
            return Ok(());
        }
    };

    match RuleRepo::new(&db).soft_delete(id).await {
        Ok(()) => {
            bot.send_message(msg.chat.id, "Alert rule deleted.").await?;
        }
        Err(_) => {
            bot.send_message(msg.chat.id, "Alert rule not found.")
                .await?;
        }
    }

    Ok(())
}

pub async fn set_rule_enabled(
    bot: Bot,
    db: Arc<Db>,
    msg: Message,
    id: i64,
    enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if auth::authorize_or_send(&bot, &db, &msg).await.is_none() {
        return Ok(());
    }

    match RuleRepo::new(&db).set_enabled(id, enabled).await {
        Ok(()) => {
            let status = if enabled { "enabled" } else { "disabled" };
            bot.send_message(msg.chat.id, format!("Alert rule {status}."))
                .await?;
        }
        Err(_) => {
            bot.send_message(msg.chat.id, "Alert rule not found.")
                .await?;
        }
    }

    Ok(())
}
