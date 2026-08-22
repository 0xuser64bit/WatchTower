//! CoinGecko-compatible price provider.
//!
//! CoinGecko's public tier accepts exactly one contract address per request and
//! rate-limits aggressively, so requests are made per mint with retry/backoff rather
//! than batched.

use crate::config::Secret;
use crate::providers::{with_retry, PriceProvider, ProviderError, ProviderResult};
use async_trait::async_trait;
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const RETRY_ATTEMPTS: u32 = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);

#[derive(Deserialize)]
struct TokenPriceResponse {
    #[serde(flatten)]
    prices: HashMap<String, CoinPrice>,
}

#[derive(Deserialize)]
struct CoinPrice {
    usd: Option<f64>,
}

pub struct CoinGeckoProvider {
    client: Client,
    base_urls: Vec<String>,
    api_key: Option<Secret>,
}

impl CoinGeckoProvider {
    /// `base_urls` is an ordered list of CoinGecko-compatible API roots; the first is
    /// primary and the rest are tried in order when it fails.
    pub fn new(
        base_urls: &[String],
        api_key: Option<Secret>,
        timeout: Duration,
    ) -> ProviderResult<Self> {
        if base_urls.is_empty() {
            return Err(ProviderError::Unavailable(
                "no price API URLs configured".into(),
            ));
        }

        let client = Client::builder()
            .timeout(timeout)
            .user_agent(concat!("chainsentinel/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(ProviderError::Http)?;

        Ok(Self {
            client,
            base_urls: base_urls
                .iter()
                .map(|url| url.trim_end_matches('/').to_string())
                .collect(),
            api_key,
        })
    }

    async fn fetch_token(&self, base_url: &str, mint: &str) -> ProviderResult<f64> {
        let mut request = self
            .client
            .get(format!("{base_url}/simple/token_price/solana"))
            .query(&[("contract_addresses", mint), ("vs_currencies", "usd")]);

        if let Some(key) = &self.api_key {
            // CoinGecko uses a different header for its paid tier.
            let header = if base_url.contains("pro-api.coingecko.com") {
                "x-cg-pro-api-key"
            } else {
                "x-cg-demo-api-key"
            };
            request = request.header(header, key.expose());
        }

        let response = request.send().await.map_err(ProviderError::Http)?;
        let response = classify_status(response)?;

        let body = response.text().await.map_err(ProviderError::Http)?;
        let parsed: TokenPriceResponse = serde_json::from_str(&body)
            .map_err(|err| ProviderError::InvalidResponse(err.to_string()))?;

        // CoinGecko echoes the contract address back as the key, but has been known
        // to normalise case for other chains. Match exactly first, then fall back.
        let price = parsed
            .prices
            .get(mint)
            .or_else(|| {
                parsed
                    .prices
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(mint))
                    .map(|(_, value)| value)
            })
            .and_then(|coin| coin.usd)
            .ok_or_else(|| {
                // An empty object means CoinGecko has no listing for this mint, which
                // is permanent rather than transient: the user's rule can never fire.
                ProviderError::Unsupported(format!("no USD price listed for mint {mint}"))
            })?;

        if !price.is_finite() || price <= 0.0 {
            return Err(ProviderError::InvalidResponse(format!(
                "non-positive price {price} for mint {mint}"
            )));
        }

        Ok(price)
    }
}

/// Maps HTTP status onto the provider error taxonomy so retryable outages and
/// deterministic response errors remain distinguishable.
fn classify_status(response: Response) -> ProviderResult<Response> {
    let status = response.status();

    if status.is_success() {
        return Ok(response);
    }

    if status == StatusCode::TOO_MANY_REQUESTS {
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs);

        return Err(ProviderError::RateLimited { retry_after });
    }

    if status.is_server_error() {
        return Err(ProviderError::Unavailable(format!("http {status}")));
    }

    Err(ProviderError::InvalidResponse(format!("http {status}")))
}

#[async_trait]
impl PriceProvider for CoinGeckoProvider {
    async fn get_token_price_usd(&self, mint: &str) -> ProviderResult<f64> {
        let mut last_error = None;

        for base_url in &self.base_urls {
            let result = with_retry("coingecko", RETRY_ATTEMPTS, RETRY_BASE_DELAY, || {
                self.fetch_token(base_url, mint)
            })
            .await;

            match result {
                Ok(price) => return Ok(price),
                // A mint the provider does not list will not be listed by a mirror of
                // the same provider either; fail fast with the actionable error.
                Err(err @ ProviderError::Unsupported(_)) => return Err(err),
                Err(err) => last_error = Some(err),
            }
        }

        Err(last_error
            .unwrap_or_else(|| ProviderError::Unavailable("no price source available".into())))
    }
}
