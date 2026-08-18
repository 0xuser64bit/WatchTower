pub mod price;
pub mod solana;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("provider unavailable: {0}")]
    Unavailable(String),

    #[error("stale data")]
    StaleData,

    #[error("rate limited")]
    RateLimited,
}

pub type ProviderResult<T> = Result<T, ProviderError>;

#[async_trait]
pub trait PriceProvider: Send + Sync {
    async fn get_native_price_usd(&self) -> ProviderResult<f64>;
    async fn get_token_price_usd(&self, mint: &str) -> ProviderResult<f64>;
}

#[async_trait]
pub trait ChainProvider: Send + Sync {
    async fn get_native_balance_lamports(&self, address: &str) -> ProviderResult<u64>;
    async fn get_token_balances(&self, owner: &str) -> ProviderResult<Vec<TokenBalance>>;
    async fn get_recent_signatures(&self, address: &str, limit: u64) -> ProviderResult<Vec<SignatureInfo>>;
    async fn get_token_decimals(&self, mint: &str) -> ProviderResult<u8>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenBalance {
    pub mint: String,
    pub amount: u64,
    pub decimals: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignatureInfo {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
}
