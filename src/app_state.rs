use crate::alerts::dispatcher::AlertDispatcher;
use crate::config::Settings;
use crate::db::Db;
use crate::engine::EngineStatus;
use crate::providers::{ChainProvider, PriceProvider};
use std::sync::Arc;
use teloxide::Bot;
use tokio_util::sync::CancellationToken;

/// Everything long-lived, shared by the control plane (Telegram) and the data plane
/// (the monitoring engine).
///
/// The Telegram layer previously received only `Arc<Db>`, which is why the configured
/// alert cooldown was ignored (the flow hardcoded 300s) and why new rules could not be
/// validated against a provider before being saved.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub bot: Bot,
    pub settings: Arc<Settings>,
    pub price_provider: Arc<dyn PriceProvider>,
    pub chain_provider: Arc<dyn ChainProvider>,
    pub dispatcher: Arc<AlertDispatcher>,
    pub status: EngineStatus,
    pub shutdown: CancellationToken,
}

impl AppState {
    pub fn new(
        db: Arc<Db>,
        bot: Bot,
        settings: Arc<Settings>,
        price_provider: Arc<dyn PriceProvider>,
        chain_provider: Arc<dyn ChainProvider>,
        shutdown: CancellationToken,
    ) -> Self {
        let dispatcher = Arc::new(AlertDispatcher::new(bot.clone(), db.clone()));

        Self {
            db,
            bot,
            settings,
            price_provider,
            chain_provider,
            dispatcher,
            status: EngineStatus::new(),
            shutdown,
        }
    }
}
