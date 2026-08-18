use crate::providers::{PriceProvider, ProviderError, ProviderResult};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone)]
pub struct CoinGeckoProvider {
    client: Client,
    base_url: String,
    fallback_urls: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SimplePriceResponse {
    solana: Option<CoinPrice>,
}

#[derive(Debug, Deserialize)]
struct CoinPrice {
    usd: f64,
}

#[derive(Debug, Deserialize)]
struct TokenPriceResponse {
    #[serde(flatten)]
    prices: std::collections::HashMap<String, CoinPrice>,
}

impl CoinGeckoProvider {
    pub fn new(base_url: &str, fallback_urls: &[String]) -> ProviderResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(ProviderError::Http)?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            fallback_urls: fallback_urls.to_vec(),
        })
    }

    fn fallback_candidates(&self, mint: &str) -> Vec<String> {
        self.fallback_urls
            .iter()
            .map(|url| url.replace("{coin}", "solana").replace("{mint}", mint))
            .collect()
    }

    async fn fetch_native(&self, url: &str) -> ProviderResult<f64> {
        let response = self
            .client
            .get(format!("{url}/simple/price"))
            .query(&[("ids", "solana"), ("vs_currencies", "usd")])
            .send()
            .await
            .map_err(ProviderError::Http)?;

        let body = response.text().await.map_err(ProviderError::Http)?;
        let parsed: SimplePriceResponse = serde_json::from_str(&body)
            .map_err(|err| ProviderError::InvalidResponse(err.to_string()))?;

        let price = parsed
            .solana
            .map(|coin| coin.usd)
            .ok_or_else(|| ProviderError::InvalidResponse("missing solana price".into()))?;

        if price <= 0.0 {
            return Err(ProviderError::InvalidResponse("non-positive price".into()));
        }

        Ok(price)
    }

    async fn fetch_token(&self, url: &str, mint: &str) -> ProviderResult<f64> {
        let response = self
            .client
            .get(format!("{url}/simple/token_price/solana"))
            .query(&[("contract_addresses", mint), ("vs_currencies", "usd")])
            .send()
            .await
            .map_err(ProviderError::Http)?;

        let body = response.text().await.map_err(ProviderError::Http)?;
        let parsed: TokenPriceResponse = serde_json::from_str(&body)
            .map_err(|err| ProviderError::InvalidResponse(err.to_string()))?;

        let price = parsed
            .prices
            .get(mint)
            .map(|coin| coin.usd)
            .ok_or_else(|| ProviderError::InvalidResponse("missing token price".into()))?;

        if price <= 0.0 {
            return Err(ProviderError::InvalidResponse("non-positive price".into()));
        }

        Ok(price)
    }
}

#[async_trait]
impl PriceProvider for CoinGeckoProvider {
    async fn get_native_price_usd(&self) -> ProviderResult<f64> {
        let mut last_err = None;

        for url in std::iter::once(self.base_url.as_str()).chain(
            self.fallback_candidates("solana")
                .iter()
                .map(String::as_str),
        ) {
            match self.fetch_native(url).await {
                Ok(price) => return Ok(price),
                Err(err) => last_err = Some(err),
            }
        }

        Err(last_err
            .unwrap_or_else(|| ProviderError::Unavailable("no price provider available".into())))
    }

    async fn get_token_price_usd(&self, mint: &str) -> ProviderResult<f64> {
        let mut last_err = None;

        for url in std::iter::once(self.base_url.as_str())
            .chain(self.fallback_candidates(mint).iter().map(String::as_str))
        {
            match self.fetch_token(url, mint).await {
                Ok(price) => return Ok(price),
                Err(err) => last_err = Some(err),
            }
        }

        Err(last_err
            .unwrap_or_else(|| ProviderError::Unavailable("no price provider available".into())))
    }
}
