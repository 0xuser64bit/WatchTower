//! `/status` — operational visibility from inside Telegram.
//!
//! Answers the questions an operator actually has during an incident: is the engine
//! polling, are the providers reachable, what failed and when. There was previously
//! no way to tell a healthy daemon from one whose monitoring loop had stopped.

use crate::alerts::format;
use crate::app_state::AppState;
use crate::db::repos::alert_events::AlertEventRepo;
use crate::db::repos::rules::RuleRepo;
use crate::db::repos::tokens::TokenRepo;
use crate::db::repos::users::UserRepo;
use crate::db::repos::wallets::WalletRepo;
use crate::telegram::flows::HandlerResult;
use crate::telegram::reply;
use chrono::Utc;
use teloxide::prelude::*;

pub async fn status(state: AppState, msg: Message) -> HandlerResult {
    if reply::require_user(&state.bot, &state.db, &msg)
        .await
        .is_none()
    {
        return Ok(());
    }

    let outcome = render(&state, &msg).await;
    reply::finish(&state.bot, msg.chat.id, "status", outcome).await
}

async fn render(state: &AppState, msg: &Message) -> crate::error::Result<()> {
    let text = build(state).await?;
    reply::send_text(&state.bot, msg.chat.id, text).await?;
    Ok(())
}

async fn build(state: &AppState) -> crate::error::Result<String> {
    let snapshot = state.status.snapshot();
    let poll_interval = state.settings.poll_interval;

    let mut lines = vec![format!(
        "ChainSentinel {} — {}",
        env!("CARGO_PKG_VERSION"),
        if snapshot.is_healthy(poll_interval) {
            "healthy"
        } else {
            "degraded"
        }
    )];

    lines.push(String::new());
    lines.push("Engine".to_string());
    lines.push(format!("  poll interval: {}s", poll_interval.as_secs()));

    match snapshot.started_at {
        Some(started) => lines.push(format!(
            "  uptime: {}",
            humanize(Utc::now().signed_duration_since(started).num_seconds())
        )),
        None => lines.push("  uptime: not started".to_string()),
    }

    match snapshot.last_tick_at {
        Some(at) => lines.push(format!(
            "  last poll: {} ago ({})",
            humanize(Utc::now().signed_duration_since(at).num_seconds()),
            format::timestamp(at)
        )),
        None => lines.push("  last poll: never".to_string()),
    }

    if let Some(duration) = snapshot.last_tick_duration {
        lines.push(format!("  last poll took: {}ms", duration.as_millis()));
    }

    lines.push(format!("  polls completed: {}", snapshot.ticks_completed));
    lines.push(format!(
        "  rules evaluated last poll: {}",
        snapshot.last_report.rules_evaluated
    ));

    if snapshot.last_report.targets_unavailable > 0 {
        lines.push(format!(
            "  targets unreadable last poll: {}",
            snapshot.last_report.targets_unavailable
        ));
    }

    lines.push(String::new());
    lines.push("Providers".to_string());
    lines.push(format!(
        "  price api: {}",
        health_label(snapshot.price_provider_healthy)
    ));
    lines.push(format!(
        "  solana rpc: {}",
        health_label(snapshot.chain_provider_healthy)
    ));
    lines.push(format!(
        "  rpc endpoints configured: {}",
        state.settings.solana_rpc_endpoints.len()
    ));

    // Database round-trips, so /status also proves persistence is working. Issued
    // sequentially: SQLite serialises anyway, and concurrency here would only
    // contend for pool connections.
    let tokens = TokenRepo::new(&state.db).count().await?;
    let wallets = WalletRepo::new(&state.db).count().await?;
    let rules_enabled = RuleRepo::new(&state.db).count_enabled().await?;
    let rules_total = RuleRepo::new(&state.db).count_all().await?;
    let events = AlertEventRepo::new(&state.db).count().await?;
    let admins = UserRepo::new(&state.db).count_active_admins().await?;

    lines.push(String::new());
    lines.push("Tracked".to_string());
    lines.push(format!("  tokens: {tokens}"));
    lines.push(format!("  wallets: {wallets}"));
    lines.push(format!(
        "  rules: {rules_enabled} enabled / {rules_total} total"
    ));

    lines.push(String::new());
    lines.push("Alerts".to_string());
    lines.push(format!(
        "  delivered since start: {}",
        snapshot.alerts_sent_total
    ));
    lines.push(format!("  history entries: {events}"));
    lines.push(format!("  recipients (active admins): {admins}"));

    if admins == 0 {
        lines.push("  WARNING: no active admin, alerts cannot be delivered".to_string());
    }

    if snapshot.consecutive_failures > 0 {
        lines.push(String::new());
        lines.push(format!(
            "Consecutive poll failures: {}",
            snapshot.consecutive_failures
        ));
    }

    if let Some(error) = &snapshot.last_error {
        lines.push(format!(
            "Last error{}: {error}",
            snapshot
                .last_error_at
                .map(|at| format!(" ({})", format::timestamp(at)))
                .unwrap_or_default()
        ));
    }

    Ok(lines.join("\n"))
}

fn health_label(healthy: Option<bool>) -> &'static str {
    match healthy {
        Some(true) => "ok",
        Some(false) => "failing",
        None => "not exercised yet",
    }
}

/// Compact duration rendering for chat output.
fn humanize(seconds: i64) -> String {
    let seconds = seconds.max(0);

    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m {}s", seconds / 60, seconds % 60),
        3600..=86_399 => format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60),
        _ => format!("{}d {}h", seconds / 86_400, (seconds % 86_400) / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanizes_across_all_scales() {
        assert_eq!(humanize(0), "0s");
        assert_eq!(humanize(45), "45s");
        assert_eq!(humanize(90), "1m 30s");
        assert_eq!(humanize(3_600), "1h 0m");
        assert_eq!(humanize(90_061), "1d 1h");
    }

    #[test]
    fn negative_durations_do_not_underflow() {
        // Possible if the host clock steps backwards between two reads.
        assert_eq!(humanize(-10), "0s");
    }

    #[test]
    fn provider_health_distinguishes_unknown_from_failing() {
        assert_eq!(health_label(None), "not exercised yet");
        assert_eq!(health_label(Some(false)), "failing");
        assert_eq!(health_label(Some(true)), "ok");
    }
}
