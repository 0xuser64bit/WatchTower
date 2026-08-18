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
    match auth::authorize(&db, &msg).await {
        Ok(_) => {}
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    }

    let rules = RuleRepo::new(&db).list_all().await?;

    if rules.is_empty() {
        bot.send_message(msg.chat.id, "No alert rules yet. Use /addalert to create one.")
            .await?;
        return Ok(());
    }

    let text = rules
        .iter()
        .map(|rule| {
            let status = if rule.is_enabled() { "enabled" } else { "disabled" };
            format!(
                "{}: {} {} {} {} ({status})",
                rule.id, rule.kind, rule.metric, rule.operator, rule.threshold
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
    match auth::authorize(&db, &msg).await {
        Ok(_) => {}
        Err(crate::error::AppError::Unauthorized) => {
            auth::send_unauthorized(&bot, msg.chat.id).await;
            return Ok(());
        }
        Err(_) => {
            let _ = bot.send_message(msg.chat.id, "Authorization failed.").await;
            return Ok(());
        }
    }

    let events = AlertEventRepo::new(&db).list_recent(10).await?;

    if events.is_empty() {
        bot.send_message(msg.chat.id, "No alerts fired yet.").await?;
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
