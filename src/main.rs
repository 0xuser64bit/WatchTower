mod config;
mod db;
mod error;

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

    info!(
        poll_interval = settings.poll_interval_seconds,
        rpc_endpoints = settings.solana_rpc_endpoints.len(),
        "ChainSentinel starting"
    );
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
