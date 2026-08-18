mod alerts;
mod app_state;
mod config;
mod db;
mod engine;
mod error;
mod providers;
mod rules;
mod telegram;

use crate::db::repos::users::{Role, UserRepo};
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
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

    let state = app_state::AppState::new(db.clone(), bot.clone(), settings.clone(), shutdown.clone());

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

    engine::scheduler::run(state, admin_chat_id).await;

    shutdown.cancel();

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

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
