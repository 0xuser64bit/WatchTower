//! Listing, toggling and deleting alert rules, plus alert history.

use crate::alerts::format;
use crate::app_state::AppState;
use crate::db::repos::alert_events::AlertEventRepo;
use crate::db::repos::rules::RuleRepo;
use crate::error::Result;
use crate::rules::types::{Operator, Rule, RuleState};
use crate::telegram::commands::parse_id;
use crate::telegram::flows::HandlerResult;
use crate::telegram::reply;
use teloxide::prelude::*;

const HISTORY_LIMIT: i64 = 15;

pub async fn list_rules(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = render_rules(&state, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "list_rules", outcome).await
}

async fn render_rules(state: &AppState, msg: &Message) -> Result<()> {
    let rules = RuleRepo::new(&state.db).list_all().await?;

    if rules.is_empty() {
        reply::send_text(
            &state.bot,
            msg.chat.id,
            "No alert rules yet. Use /addalert to create one.",
        )
        .await?;
        return Ok(());
    }

    let body = rules
        .iter()
        .map(render_rule)
        .collect::<Vec<_>>()
        .join("\n\n");

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("Alert rules ({}):\n\n{body}", rules.len()),
    )
    .await?;
    Ok(())
}

fn render_rule(rule: &Rule) -> String {
    let status = match (rule.enabled, rule.state) {
        (false, _) => "disabled",
        (true, RuleState::Firing) => "firing",
        (true, RuleState::Ok) => "armed",
    };

    let mut lines = vec![
        format!("{}. {} [{status}]", rule.id, rule.target.display()),
        format!("   {}", rule.condition()),
    ];

    if let Some(last) = rule.last_value {
        lines.push(format!(
            "   last seen {} {}",
            format::amount(last),
            rule.target.kind.unit()
        ));
    }

    if rule.operator.is_percentage() {
        // Without this the user has no way to know what a percentage rule is
        // currently measuring against.
        match rule.reference_value {
            Some(baseline) => lines.push(format!(
                "   baseline {} {}",
                format::amount(baseline),
                rule.target.kind.unit()
            )),
            None => lines.push("   baseline not set yet".to_string()),
        }
    }

    if let Some(at) = rule.last_triggered_at {
        lines.push(format!("   last fired {}", format::timestamp(at)));
    }

    lines.join("\n")
}

pub async fn set_enabled(
    state: AppState,
    msg: Message,
    args: String,
    enabled: bool,
) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let command = if enabled { "enablerule" } else { "disablerule" };
    let Some(id) = parse_id(&state, &msg, &args, &format!("/{command} <id>")).await else {
        return Ok(());
    };

    let outcome = toggle_rule(&state, &msg, id, enabled).await;
    reply::finish(&state.bot, msg.chat.id, "set_rule_enabled", outcome).await
}

async fn toggle_rule(state: &AppState, msg: &Message, id: i64, enabled: bool) -> Result<()> {
    let rule = RuleRepo::new(&state.db).set_enabled(id, enabled).await?;

    let verb = if enabled { "enabled" } else { "disabled" };
    let note = if enabled && rule.operator.is_percentage() {
        " Its baseline will be taken from the next observation."
    } else {
        ""
    };

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "Rule {} ({} {}) {verb}.{note}",
            rule.id,
            rule.target.display(),
            rule.condition()
        ),
    )
    .await?;
    Ok(())
}

pub async fn delete_rule(state: AppState, msg: Message, args: String) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let Some(id) = parse_id(&state, &msg, &args, "/deleterule <id>").await else {
        return Ok(());
    };

    let outcome = remove_rule(&state, &msg, id).await;
    reply::finish(&state.bot, msg.chat.id, "delete_rule", outcome).await
}

async fn remove_rule(state: &AppState, msg: &Message, id: i64) -> Result<()> {
    let repo = RuleRepo::new(&state.db);

    let Some(rule) = repo.find(id).await? else {
        reply::send_text(
            &state.bot,
            msg.chat.id,
            format!("No alert rule with id {id}."),
        )
        .await?;
        return Ok(());
    };

    repo.delete(id).await?;

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!(
            "Deleted rule {} ({} {}). Past alerts stay in /history.",
            rule.id,
            rule.target.display(),
            rule.condition()
        ),
    )
    .await?;
    Ok(())
}

pub async fn history(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = render_history(&state, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "history", outcome).await
}

async fn render_history(state: &AppState, msg: &Message) -> Result<()> {
    let events = AlertEventRepo::new(&state.db)
        .list_recent(HISTORY_LIMIT)
        .await?;

    if events.is_empty() {
        reply::send_text(&state.bot, msg.chat.id, "No alerts have fired yet.").await?;
        return Ok(());
    }

    // Render from structured columns so each event remains compact and readable.
    let body = events
        .iter()
        .map(|event| {
            let unit = event.kind().map(|kind| kind.unit()).unwrap_or("");
            let target = event
                .target_label
                .clone()
                .unwrap_or_else(|| crate::rules::types::abbreviate(&event.target_ref));

            let comparison = match Operator::parse(&event.operator) {
                Some(operator) if operator.is_percentage() => match event.reference_value {
                    Some(baseline) => format::change_pct(event.observed_value, baseline)
                        .map(|pct| {
                            format!("{} from {}", format::percent(pct), format::amount(baseline))
                        })
                        .unwrap_or_else(|| format!("{}%", format::amount(event.threshold_value))),
                    None => format!("{}%", format::amount(event.threshold_value)),
                },
                Some(operator) => format!(
                    "{} {}",
                    operator.symbol(),
                    format::amount(event.threshold_value)
                ),
                None => format::amount(event.threshold_value),
            };

            format!(
                "{} — {target}\n   {} {unit} ({comparison})",
                format::timestamp(event.triggered_at),
                format::amount(event.observed_value),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    reply::send_text(
        &state.bot,
        msg.chat.id,
        format!("Recent alerts (newest first):\n\n{body}"),
    )
    .await?;
    Ok(())
}
