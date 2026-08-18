mod alerts;
mod config;
mod db;
mod error;
mod providers;
mod rules;
mod telegram;

use crate::db::repos::users::{Role, UserRepo};
use std::sync::Arc;
use teloxide::prelude::*;
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

    info!(
        poll_interval = settings.poll_interval_seconds,
        rpc_endpoints = settings.solana_rpc_endpoints.len(),
        "ChainSentinel starting"
    );

    telegram::run(bot, db).await;
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
