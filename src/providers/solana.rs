//! Solana JSON-RPC provider.

use crate::config::Commitment;
use crate::providers::{with_retry, ChainProvider, ProviderError, ProviderResult};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// How long an endpoint stays benched after a failure.
const HEALTH_RE_ENABLE_AFTER: Duration = Duration::from_secs(60);
/// `getMultipleAccounts` accepts at most 100 keys per call.
const MAX_ACCOUNTS_PER_CALL: usize = 100;
const RETRY_ATTEMPTS: u32 = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_millis(300);

/// Validates a Solana address: base58 that decodes to exactly 32 bytes.
pub fn is_valid_address(raw: &str) -> bool {
    if raw.len() < 32 || raw.len() > 44 {
        return false;
    }

    matches!(bs58::decode(raw).into_vec(), Ok(decoded) if decoded.len() == 32)
}

pub struct SolanaRpcProvider {
    client: Client,
    endpoints: Arc<Vec<RpcEndpoint>>,
    next_index: Arc<AtomicUsize>,
    commitment: Commitment,
}

#[derive(Debug)]
struct RpcEndpoint {
    url: String,
    /// `None` when healthy, otherwise when it last failed.
    benched_at: Mutex<Option<Instant>>,
}

#[derive(Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcErrorBody>,
}

#[derive(Deserialize)]
struct RpcErrorBody {
    code: i64,
    message: String,
}

#[derive(Deserialize)]
struct ValueResult<T> {
    value: T,
}

#[derive(Deserialize)]
struct AccountInfo {
    lamports: u64,
}

impl SolanaRpcProvider {
    pub fn new(
        endpoints: Vec<String>,
        commitment: Commitment,
        timeout: Duration,
    ) -> ProviderResult<Self> {
        if endpoints.is_empty() {
            return Err(ProviderError::Unavailable(
                "no RPC endpoints configured".into(),
            ));
        }

        let client = Client::builder()
            .timeout(timeout)
            .user_agent(concat!("watchtower/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(ProviderError::Http)?;

        let endpoints = endpoints
            .into_iter()
            .map(|url| RpcEndpoint {
                url,
                benched_at: Mutex::new(None),
            })
            .collect();

        Ok(Self {
            client,
            endpoints: Arc::new(endpoints),
            next_index: Arc::new(AtomicUsize::new(0)),
            commitment,
        })
    }

    /// Round-robins over endpoints, skipping ones benched within the cool-off window.
    fn select_endpoint(&self) -> ProviderResult<usize> {
        let len = self.endpoints.len();

        for _ in 0..len {
            let index = self.next_index.fetch_add(1, Ordering::Relaxed) % len;

            // A poisoned lock only means some other thread panicked while marking
            // health; the value is still meaningful, so recover rather than abort.
            let mut benched = lock(&self.endpoints[index].benched_at);

            match *benched {
                None => return Ok(index),
                Some(at) if at.elapsed() >= HEALTH_RE_ENABLE_AFTER => {
                    *benched = None;
                    debug!(endpoint = %self.endpoints[index].url, "re-enabled RPC endpoint");
                    return Ok(index);
                }
                Some(_) => continue,
            }
        }

        Err(ProviderError::Unavailable(
            "all RPC endpoints are unhealthy".into(),
        ))
    }

    fn bench(&self, index: usize, reason: &str) {
        // With one endpoint there is nothing to fail over to, so benching it would
        // only guarantee an "all endpoints unhealthy" error for the next minute.
        if self.endpoints.len() == 1 {
            warn!(endpoint = %self.endpoints[index].url, reason, "RPC request failed");
            return;
        }

        warn!(endpoint = %self.endpoints[index].url, reason, "benching RPC endpoint");
        *lock(&self.endpoints[index].benched_at) = Some(Instant::now());
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> ProviderResult<T> {
        with_retry("solana-rpc", RETRY_ATTEMPTS, RETRY_BASE_DELAY, || {
            self.call_once(method, params.clone())
        })
        .await
    }

    async fn call_once<T: for<'de> Deserialize<'de>>(
        &self,
        method: &'static str,
        params: serde_json::Value,
    ) -> ProviderResult<T> {
        let index = self.select_endpoint()?;
        let url = &self.endpoints[index].url;

        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });

        let response = match self.client.post(url).json(&body).send().await {
            Ok(response) => response,
            Err(err) => {
                self.bench(index, &err.to_string());
                return Err(ProviderError::Http(err));
            }
        };

        let status = response.status();

        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs);

            self.bench(index, "rate limited");
            return Err(ProviderError::RateLimited { retry_after });
        }

        let text = response.text().await.map_err(ProviderError::Http)?;

        if !status.is_success() {
            self.bench(index, &format!("http {status}"));
            return Err(ProviderError::Unavailable(format!("http {status}")));
        }

        let parsed: RpcResponse<T> = serde_json::from_str(&text)
            .map_err(|err| ProviderError::InvalidResponse(err.to_string()))?;

        if let Some(error) = parsed.error {
            // -32602 is "invalid params": a malformed address, not an unhealthy
            // endpoint. Benching for it would take a healthy node out of rotation
            // because of bad user input.
            if error.code == -32602 {
                return Err(ProviderError::InvalidResponse(error.message));
            }

            self.bench(index, &error.message);
            return Err(ProviderError::Unavailable(error.message));
        }

        parsed
            .result
            .ok_or_else(|| ProviderError::InvalidResponse(format!("{method} returned no result")))
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[async_trait]
impl ChainProvider for SolanaRpcProvider {
    async fn get_native_balance_lamports(&self, address: &str) -> ProviderResult<u64> {
        if !is_valid_address(address) {
            return Err(ProviderError::InvalidResponse(format!(
                "`{address}` is not a valid Solana address"
            )));
        }

        let params = json!([address, { "commitment": self.commitment.as_str() }]);
        let result: ValueResult<u64> = self.call("getBalance", params).await?;
        Ok(result.value)
    }

    async fn get_native_balances_lamports(&self, addresses: &[String]) -> ProviderResult<Vec<u64>> {
        let mut balances = Vec::with_capacity(addresses.len());

        for chunk in addresses.chunks(MAX_ACCOUNTS_PER_CALL) {
            let params = json!([
                chunk,
                {
                    "commitment": self.commitment.as_str(),
                    "encoding": "base64",
                    // Balances only: skip transferring account data entirely.
                    "dataSlice": { "offset": 0, "length": 0 }
                }
            ]);

            let result: ValueResult<Vec<Option<AccountInfo>>> =
                self.call("getMultipleAccounts", params).await?;

            if result.value.len() != chunk.len() {
                return Err(ProviderError::InvalidResponse(format!(
                    "getMultipleAccounts returned {} entries for {} addresses",
                    result.value.len(),
                    chunk.len()
                )));
            }

            // A null entry means the account does not exist on chain, which on
            // Solana is exactly a zero balance — not an unknown value.
            balances.extend(
                result
                    .value
                    .into_iter()
                    .map(|account| account.map_or(0, |account| account.lamports)),
            );
        }

        Ok(balances)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_real_32_byte_address() {
        assert!(is_valid_address(
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        ));
        assert!(is_valid_address(&bs58::encode([7u8; 32]).into_string()));
    }

    #[test]
    fn rejects_wrong_length_or_alphabet() {
        assert!(!is_valid_address("short"));
        assert!(!is_valid_address(""));
        // 0, O, I and l are not in the base58 alphabet.
        assert!(!is_valid_address("0OIl0OIl0OIl0OIl0OIl0OIl0OIl0OIl"));
        // Decodes as base58 but to the wrong number of bytes.
        assert!(!is_valid_address(&bs58::encode([7u8; 31]).into_string()));
    }
}
