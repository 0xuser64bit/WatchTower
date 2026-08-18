pub mod alerts;
pub mod app_state;
pub mod config;
pub mod db;
pub mod engine;
pub mod error;
pub mod providers;
pub mod rules;
pub mod telegram;

use db::repos::users::{Role, UserRepo};
use providers::price::coingecko::CoinGeckoProvider;
use providers::solana::rpc::SolanaRpcProvider;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::EnvFilter;

pub fn main_impl() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    rt.block_on(async_main());
}

async fn async_main() {
    init_logging();

    let settings = match config::Settings::load() {
        Ok(settings) => settings,
        Err(err) => {
            tracing::error!(%err, "failed to load configuration");
            std::process::exit(1);
        }
    };

    let db = match db::Db::connect(&settings.database_url).await {
        Ok(db) => db,
        Err(err) => {
            tracing::error!(%err, "failed to connect to database");
            std::process::exit(1);
        }
    };

    if let Err(err) = db.migrate().await {
        tracing::error!(%err, "failed to run database migrations");
        std::process::exit(1);
    }

    if let Err(err) = seed_admins(&db, &settings.admin_telegram_ids).await {
        tracing::error!(%err, "failed to seed admin users");
        std::process::exit(1);
    }

    let db = Arc::new(db);
    let bot = Bot::new(&settings.telegram_bot_token);
    let settings = Arc::new(settings);
    let shutdown = CancellationToken::new();

    let price_provider =
        match CoinGeckoProvider::new(&settings.coingecko_api_url, &settings.price_fallback_urls) {
            Ok(provider) => Arc::new(provider),
            Err(err) => {
                tracing::error!(%err, "failed to build price provider");
                std::process::exit(1);
            }
        };

    let chain_provider = match SolanaRpcProvider::new(
        settings.solana_rpc_endpoints.clone(),
        &settings.solana_rpc_commitment,
    ) {
        Ok(provider) => Arc::new(provider),
        Err(err) => {
            tracing::error!(%err, "failed to build Solana RPC provider");
            std::process::exit(1);
        }
    };

    let state = app_state::AppState::new(
        db.clone(),
        bot.clone(),
        settings.clone(),
        price_provider,
        chain_provider,
        shutdown.clone(),
    );

    let admin_chat_id = ChatId(settings.admin_telegram_ids[0]);

    info!(
        poll_interval = settings.poll_interval_seconds,
        rpc_endpoints = settings.solana_rpc_endpoints.len(),
        "ChainSentinel starting"
    );

    let bot_shutdown = shutdown.clone();
    let bot_task = tokio::spawn(async move {
        telegram::run(bot, db, bot_shutdown).await;
    });

    let signal_shutdown = shutdown.clone();
    let signal_task = tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        signal_shutdown.cancel();
    });

    engine::scheduler::run(state, admin_chat_id).await;

    shutdown.cancel();
    let _ = signal_task.await;

    if let Err(err) = bot_task.await {
        tracing::error!(%err, "telegram task failed");
    }

    info!("ChainSentinel stopped");
}

async fn seed_admins(db: &db::Db, admin_ids: &[i64]) -> crate::error::Result<()> {
    let repo = UserRepo::new(db);

    for telegram_id in admin_ids {
        if repo.find_by_telegram_id(*telegram_id).await?.is_none() {
            repo.create(*telegram_id, Role::Admin).await?;
            info!(telegram_id, "seeded admin user");
        }
    }

    Ok(())
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(err) => {
                tracing::warn!(%err, "failed to install SIGTERM handler");
                return;
            }
        };

        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(signal) => signal,
            Err(err) => {
                tracing::warn!(%err, "failed to install SIGINT handler");
                return;
            }
        };

        tokio::select! {
            _ = sigterm.recv() => tracing::info!("received SIGTERM"),
            _ = sigint.recv() => tracing::info!("received SIGINT"),
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::warn!(%err, "failed to install Ctrl+C handler");
        }
    }
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
