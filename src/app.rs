//! Process bootstrap: build everything, run it, shut it down cleanly.

use crate::app_state::AppState;
use crate::config::Settings;
use crate::db::repos::users::{Role, UserRepo};
use crate::db::Db;
use crate::error::{AppError, Result};
use crate::providers::price::CoinGeckoProvider;
use crate::providers::solana::SolanaRpcProvider;
use crate::{engine, observability, telegram};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// How long in-flight work gets to finish after a shutdown signal.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(20);

/// Why the process is stopping. Determines the exit code, so a supervisor and an
/// operator can tell a requested shutdown from a component failure. Previously any
/// outcome, including the control plane dying at startup, exited zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// SIGTERM or SIGINT.
    Signal,
    /// The control plane or the monitoring engine ended on its own.
    TaskFailed,
}

pub fn main() -> std::process::ExitCode {
    // Configuration is read before logging is initialised, so failures here have to
    // go to stderr directly.
    let settings = match Settings::load() {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("chainsentinel: configuration error: {err}");
            eprintln!("chainsentinel: see .env.example for the expected variables");
            return std::process::ExitCode::FAILURE;
        }
    };

    let _logging = observability::init(&settings.log_dir, settings.log_max_files);

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            error!(%err, "failed to build the tokio runtime");
            return std::process::ExitCode::FAILURE;
        }
    };

    match runtime.block_on(run(settings)) {
        Ok(Stop::Signal) => std::process::ExitCode::SUCCESS,
        Ok(Stop::TaskFailed) => std::process::ExitCode::FAILURE,
        Err(err) => {
            error!(%err, "chainsentinel failed to start");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run(settings: Settings) -> Result<Stop> {
    info!(
        version = env!("CARGO_PKG_VERSION"),
        poll_interval_secs = settings.poll_interval.as_secs(),
        rpc_endpoints = settings.solana_rpc_endpoints.len(),
        price_endpoints = settings.coingecko_api_urls.len(),
        commitment = settings.solana_rpc_commitment.as_str(),
        "starting ChainSentinel"
    );

    let db = Db::connect(&settings.database_url).await?;
    db.migrate().await?;
    db.ping().await?;
    info!(database = %redact_path(&settings.database_url), "database ready");

    seed_admins(&db, &settings.admin_telegram_ids).await?;

    let price_provider = Arc::new(CoinGeckoProvider::new(
        &settings.coingecko_api_urls,
        settings.coingecko_api_key.clone(),
        settings.http_timeout,
    )?);

    let chain_provider = Arc::new(SolanaRpcProvider::new(
        settings.solana_rpc_endpoints.clone(),
        settings.solana_rpc_commitment,
        settings.http_timeout,
    )?);

    let bot = teloxide::Bot::new(settings.telegram_bot_token.expose());

    // Verify the credential before starting anything else. teloxide's dispatcher
    // calls `getMe` internally and `expect`s the result, so a bad token otherwise
    // surfaces as a panic from inside a dependency with no actionable context.
    verify_bot_token(&bot).await?;

    let shutdown = CancellationToken::new();

    let state = AppState::new(
        Arc::new(db.clone()),
        bot,
        Arc::new(settings),
        price_provider,
        chain_provider,
        shutdown.clone(),
    );

    let mut tasks = tokio::task::JoinSet::new();

    let telegram_state = state.clone();
    tasks.spawn(async move { telegram::run(telegram_state).await });

    let engine_state = state.clone();
    tasks.spawn(async move { engine::scheduler::run(engine_state).await });

    // Either half dying leaves the daemon unusable: a running engine with a dead
    // control plane cannot be managed or even inspected. Stop, and exit non-zero so a
    // supervisor restart is visible rather than looking like a clean shutdown.
    let stop = tokio::select! {
        _ = wait_for_shutdown_signal() => Stop::Signal,
        Some(result) = tasks.join_next() => {
            match result {
                Ok(()) => error!("a core task stopped unexpectedly"),
                Err(err) => error!(%err, "a core task terminated abnormally"),
            }
            Stop::TaskFailed
        }
    };

    shutdown.cancel();
    drain(&mut tasks).await;

    if let Err(err) = db.checkpoint().await {
        warn!(%err, "failed to checkpoint the write-ahead log");
    }
    db.close().await;

    info!(?stop, "ChainSentinel stopped");
    Ok(stop)
}

/// Waits for the remaining tasks, then forces them down if they overrun.
async fn drain(tasks: &mut tokio::task::JoinSet<()>) {
    let wait = async { while tasks.join_next().await.is_some() {} };

    if tokio::time::timeout(SHUTDOWN_GRACE, wait).await.is_err() {
        // A Telegram long-poll can sit on an open connection for its full timeout.
        warn!(
            grace_secs = SHUTDOWN_GRACE.as_secs(),
            "shutdown grace period elapsed; aborting remaining tasks"
        );
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
}

async fn verify_bot_token(bot: &teloxide::Bot) -> Result<()> {
    use teloxide::prelude::Requester;

    match bot.get_me().await {
        Ok(me) => {
            info!(
                bot_username = me.user.username.as_deref().unwrap_or("unknown"),
                bot_id = me.user.id.0,
                "authenticated with Telegram"
            );
            Ok(())
        }
        Err(err) => Err(AppError::InvalidInput(format!(
            "Telegram rejected the bot token ({err}). Check TELEGRAM_BOT_TOKEN against @BotFather"
        ))),
    }
}

/// Ensures the configured bootstrap admins exist.
///
/// Only creates missing users. It deliberately does not re-promote an id that was
/// later demoted or blocked through the bot: the database is the authority, and a
/// stale entry in `.env` must not silently restore access on the next restart.
async fn seed_admins(db: &Db, admin_ids: &[i64]) -> Result<()> {
    let repo = UserRepo::new(db);

    for telegram_id in admin_ids {
        if repo.find_by_telegram_id(*telegram_id).await?.is_none() {
            repo.upsert(*telegram_id, Role::Admin).await?;
            info!(telegram_id, "seeded bootstrap admin");
        }
    }

    if repo.count_active_admins().await? == 0 {
        // Not fatal: an operator may be mid-rotation. But alerting has no recipients
        // and nobody can manage the bot, so it must be impossible to miss.
        warn!(
            "no active admin exists; alerts cannot be delivered and admin commands \
             are unavailable until one is restored"
        );
    }

    Ok(())
}

/// Strips a filesystem path down to its file name for logging.
fn redact_path(database_url: &str) -> String {
    database_url
        .rsplit('/')
        .next()
        .unwrap_or(database_url)
        .to_string()
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(err) => {
                warn!(%err, "failed to install SIGTERM handler");
                return std::future::pending().await;
            }
        };

        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(signal) => signal,
            Err(err) => {
                warn!(%err, "failed to install SIGINT handler");
                return std::future::pending().await;
            }
        };

        tokio::select! {
            _ = sigterm.recv() => info!("received SIGTERM"),
            _ = sigint.recv() => info!("received SIGINT"),
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(err) = tokio::signal::ctrl_c().await {
            warn!(%err, "failed to install Ctrl+C handler");
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_url_is_reduced_to_a_file_name_for_logs() {
        assert_eq!(redact_path("sqlite:///srv/secret/app.db"), "app.db");
        assert_eq!(redact_path("sqlite::memory:"), "sqlite::memory:");
    }

    #[tokio::test]
    async fn seeding_is_idempotent_and_does_not_repromote() {
        let db = Db::connect_in_memory().await.unwrap();
        db.migrate().await.unwrap();

        seed_admins(&db, &[1, 2]).await.unwrap();
        let repo = UserRepo::new(&db);
        assert_eq!(repo.count_active_admins().await.unwrap(), 2);

        // An admin demoted through the bot must stay demoted across restarts.
        repo.set_role(2, Role::User).await.unwrap();
        seed_admins(&db, &[1, 2]).await.unwrap();

        assert_eq!(
            repo.find_by_telegram_id(2).await.unwrap().unwrap().role,
            Role::User
        );
        assert_eq!(repo.count_active_admins().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn seeding_repeatedly_does_not_duplicate_users() {
        let db = Db::connect_in_memory().await.unwrap();
        db.migrate().await.unwrap();

        for _ in 0..3 {
            seed_admins(&db, &[42]).await.unwrap();
        }

        assert_eq!(UserRepo::new(&db).list().await.unwrap().len(), 1);
    }
}
