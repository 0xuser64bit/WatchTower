use crate::providers::{ChainProvider, ProviderError, ProviderResult};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

const HEALTH_RE_ENABLE_AFTER: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct SolanaRpcProvider {
    client: Client,
    endpoints: Arc<Vec<RpcEndpoint>>,
    next_index: Arc<AtomicUsize>,
    commitment: String,
}

#[derive(Debug)]
struct RpcEndpoint {
    url: String,
    healthy: Mutex<bool>,
    last_failure: Mutex<Option<Instant>>,
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcErrorBody>,
}

#[derive(Debug, Deserialize)]
struct RpcErrorBody {
    message: String,
}

#[derive(Debug, Deserialize)]
struct BalanceResult {
    value: u64,
}

impl SolanaRpcProvider {
    pub fn new(endpoints: Vec<String>, commitment: &str) -> ProviderResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(ProviderError::Http)?;

        let rpc_endpoints = endpoints
            .into_iter()
            .map(|url| RpcEndpoint {
                url,
                healthy: Mutex::new(true),
                last_failure: Mutex::new(None),
            })
            .collect();

        Ok(Self {
            client,
            endpoints: Arc::new(rpc_endpoints),
            next_index: Arc::new(AtomicUsize::new(0)),
            commitment: commitment.to_string(),
        })
    }

    fn select_endpoint(&self) -> ProviderResult<usize> {
        let len = self.endpoints.len();
        if len == 0 {
            return Err(ProviderError::Unavailable("no RPC endpoints configured".into()));
        }

        for _ in 0..len {
            let index = self.next_index.fetch_add(1, Ordering::Relaxed) % len;
            let endpoint = &self.endpoints[index];

            if *endpoint.healthy.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) {
                return Ok(index);
            }

            self.maybe_re_enable(index);
        }

        Err(ProviderError::Unavailable("all RPC endpoints are unhealthy".into()))
    }

    fn maybe_re_enable(&self, index: usize) {
        let endpoint = &self.endpoints[index];

        let Ok(mut healthy) = endpoint.healthy.lock() else {
            return;
        };

        if *healthy {
            return;
        }

        let last_failure = *endpoint
            .last_failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(last) = last_failure {
            if last.elapsed() < HEALTH_RE_ENABLE_AFTER {
                return;
            }
        }

        *healthy = true;
        debug!(endpoint = %endpoint.url, "re-enabled RPC endpoint");
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> ProviderResult<T> {
        let index = self.select_endpoint()?;
        let endpoint = &self.endpoints[index];

        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let response = match self.client.post(&endpoint.url).json(&body).send().await {
            Ok(response) => response,
            Err(err) => {
                let message = err.to_string();
                self.mark_failed(index, &message);
                return Err(ProviderError::Http(err));
            }
        };

        let status = response.status();
        let text = response.text().await.map_err(ProviderError::Http)?;

        if !status.is_success() {
            let message = format!("http status {status}");
            self.mark_failed(index, &message);
            return Err(ProviderError::Unavailable(message));
        }

        let parsed: RpcResponse<T> = serde_json::from_str(&text)
            .map_err(|err| ProviderError::InvalidResponse(err.to_string()))?;

        if let Some(err) = parsed.error {
            let message = err.message.clone();
            self.mark_failed(index, &message);
            return Err(ProviderError::Unavailable(err.message));
        }

        let result = parsed
            .result
            .ok_or_else(|| ProviderError::InvalidResponse("missing rpc result".into()))?;

        Ok(result)
    }

    fn mark_failed(&self, index: usize, message: &str) {
        let endpoint = &self.endpoints[index];
        warn!(endpoint = %endpoint.url, %message, "marking RPC endpoint unhealthy");

        if let Ok(mut healthy) = endpoint.healthy.lock() {
            *healthy = false;
        }

        if let Ok(mut last_failure) = endpoint.last_failure.lock() {
            *last_failure = Some(Instant::now());
        }
    }
}

#[async_trait]
impl ChainProvider for SolanaRpcProvider {
    async fn get_native_balance_lamports(&self, address: &str) -> ProviderResult<u64> {
        let params = json!([address, {
            "commitment": self.commitment,
        }]);

        let result: BalanceResult = self.call("getBalance", params).await?;
        Ok(result.value)
    }
}
