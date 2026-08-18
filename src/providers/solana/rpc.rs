use crate::providers::solana::parse::{decode_base64_account, parse_token_account};
use crate::providers::{
    ChainProvider, ProviderError, ProviderResult, SignatureInfo, TokenBalance,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
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

#[derive(Debug, Deserialize)]
struct TokenAccountsResult {
    value: Vec<TokenAccount>,
}

#[derive(Debug, Deserialize)]
struct TokenAccount {
    account: RpcAccount,
}

#[derive(Debug, Deserialize)]
struct RpcAccount {
    data: AccountData,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AccountData {
    Encoded { encoded: String },
    Parsed,
}

#[derive(Debug, Deserialize)]
struct MintAccountInfo {
    data: MintAccountData,
}

#[derive(Debug, Deserialize)]
struct MintAccountData {
    #[serde(default)]
    parsed: Option<MintParsed>,
}

#[derive(Debug, Deserialize)]
struct MintParsed {
    info: MintInfo,
}

#[derive(Debug, Deserialize)]
struct MintInfo {
    decimals: u8,
}

#[derive(Debug, Deserialize)]
struct SignaturesResult {
    value: Vec<RpcSignature>,
}

#[derive(Debug, Deserialize)]
struct RpcSignature {
    signature: String,
    slot: u64,
    #[serde(rename = "blockTime")]
    block_time: Option<i64>,
}

impl SolanaRpcProvider {
    pub fn new(endpoints: Vec<String>, commitment: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client");

        let rpc_endpoints = endpoints
            .into_iter()
            .map(|url| RpcEndpoint {
                url,
                healthy: Mutex::new(true),
                last_failure: Mutex::new(None),
            })
            .collect();

        Self {
            client,
            endpoints: Arc::new(rpc_endpoints),
            next_index: Arc::new(AtomicUsize::new(0)),
            commitment: commitment.to_string(),
        }
    }

    fn select_endpoint(&self) -> ProviderResult<usize> {
        let len = self.endpoints.len();
        if len == 0 {
            return Err(ProviderError::Unavailable("no RPC endpoints configured".into()));
        }

        for _ in 0..len {
            let index = self.next_index.fetch_add(1, Ordering::Relaxed) % len;
            let endpoint = &self.endpoints[index];

            if *endpoint.healthy.lock().unwrap() {
                return Ok(index);
            }

            self.maybe_re_enable(index);
        }

        Err(ProviderError::Unavailable("all RPC endpoints are unhealthy".into()))
    }

    fn maybe_re_enable(&self, index: usize) {
        let endpoint = &self.endpoints[index];

        if let Ok(mut healthy) = endpoint.healthy.lock() {
            if *healthy {
                return;
            }

            if let Some(last) = *endpoint.last_failure.lock().unwrap() {
                if last.elapsed() < HEALTH_RE_ENABLE_AFTER {
                    return;
                }
            }

            *healthy = true;
            debug!(endpoint = %endpoint.url, "re-enabled RPC endpoint");
        }
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

    async fn get_token_balances(&self, owner: &str) -> ProviderResult<Vec<TokenBalance>> {
        let mut balances = Vec::new();

        for program_id in [TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID] {
            let params = json!([
                owner,
                {
                    "programId": program_id,
                    "encoding": "base64",
                },
                {
                    "commitment": self.commitment,
                }
            ]);

            let result: TokenAccountsResult = self.call("getTokenAccountsByOwner", params).await?;

            for account in result.value {
                let data = match &account.account.data {
                    AccountData::Encoded { encoded } => decode_base64_account(encoded)
                        .ok_or_else(|| ProviderError::InvalidResponse("invalid base64 account data".into()))?,
                    AccountData::Parsed => {
                        return Err(ProviderError::InvalidResponse(
                            "unexpected parsed account encoding".into(),
                        ))
                    }
                };

                let (mint, amount) = parse_token_account(&data).ok_or_else(|| {
                    ProviderError::InvalidResponse("invalid token account layout".into())
                })?;

                balances.push(TokenBalance {
                    mint,
                    amount,
                    decimals: 0,
                });
            }
        }

        Ok(balances)
    }

    async fn get_recent_signatures(
        &self,
        address: &str,
        limit: u64,
    ) -> ProviderResult<Vec<SignatureInfo>> {
        let params = json!([address, {
            "limit": limit,
            "commitment": self.commitment,
        }]);

        let result: SignaturesResult = self.call("getSignaturesForAddress", params).await?;

        Ok(result
            .value
            .into_iter()
            .map(|sig| SignatureInfo {
                signature: sig.signature,
                slot: sig.slot,
                block_time: sig.block_time,
            })
            .collect())
    }

    async fn get_token_decimals(&self, mint: &str) -> ProviderResult<u8> {
        let params = json!([mint, {
            "encoding": "jsonParsed",
            "commitment": self.commitment,
        }]);

        let result: MintAccountInfo = self.call("getAccountInfo", params).await?;

        let decimals = result
            .data
            .parsed
            .ok_or_else(|| ProviderError::InvalidResponse("missing parsed mint info".into()))?
            .info
            .decimals;

        Ok(decimals)
    }
}
