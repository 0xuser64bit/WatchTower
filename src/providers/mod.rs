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
}
