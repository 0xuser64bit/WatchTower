use crate::config::Settings;
use crate::db::Db;
use crate::providers::price::coingecko::CoinGeckoProvider;
use crate::providers::solana::rpc::SolanaRpcProvider;
use crate::alerts::dispatcher::AlertDispatcher;
use std::sync::Arc;
use teloxide::prelude::*;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub bot: Bot,
    pub settings: Arc<Settings>,
    pub price_provider: Arc<CoinGeckoProvider>,
    pub chain_provider: Arc<SolanaRpcProvider>,
    pub dispatcher: Arc<AlertDispatcher>,
    pub shutdown: CancellationToken,
}

impl AppState {
    pub fn new(
        db: Arc<Db>,
        bot: Bot,
        settings: Arc<Settings>,
        shutdown: CancellationToken,
    ) -> Self {
        let price_provider = Arc::new(
            CoinGeckoProvider::new(&settings.coingecko_api_url, &settings.price_fallback_urls)
                .expect("failed to build price provider"),
        );

        let chain_provider = Arc::new(SolanaRpcProvider::new(
            settings.solana_rpc_endpoints.clone(),
            &settings.solana_rpc_commitment,
        ));

        let dispatcher = Arc::new(AlertDispatcher::new(bot.clone(), db.clone()));

        Self {
            db,
            bot,
            settings,
            price_provider,
            chain_provider,
            dispatcher,
            shutdown,
        }
    }
}
