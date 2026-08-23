//! Network checks used by the setup wizard. Errors never include secrets or URLs
//! that embed a bot token.

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotInfo {
    pub id: i64,
    pub username: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LiveCheckError {
    #[error("{0}")]
    Unreachable(String),
    #[error("{0}")]
    Rejected(String),
}

#[async_trait]
pub trait LiveChecker: Send + Sync {
    async fn telegram_get_me(&self, token: &str) -> Result<BotInfo, LiveCheckError>;
    async fn ping_coingecko(
        &self,
        base_url: &str,
        api_key: Option<&str>,
    ) -> Result<(), LiveCheckError>;
    async fn ping_solana_rpc(&self, url: &str) -> Result<(), LiveCheckError>;
}

pub struct HttpLiveChecker {
    client: Client,
    telegram_api_base: String,
}

impl HttpLiveChecker {
    pub fn new(timeout: Duration) -> Result<Self, reqwest::Error> {
        Self::with_telegram_base(timeout, "https://api.telegram.org")
    }

    pub fn with_telegram_base(
        timeout: Duration,
        telegram_api_base: &str,
    ) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .timeout(timeout)
            .user_agent(concat!("watchtower/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            client,
            telegram_api_base: telegram_api_base.trim_end_matches('/').to_string(),
        })
    }
}

#[derive(Deserialize)]
struct TelegramResponse {
    ok: bool,
    result: Option<TelegramUser>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct TelegramUser {
    id: i64,
    username: Option<String>,
    first_name: Option<String>,
}

#[derive(Deserialize)]
struct RpcResponse {
    result: Option<serde_json::Value>,
    error: Option<RpcErrorBody>,
}

#[derive(Deserialize)]
struct RpcErrorBody {
    message: String,
}

#[async_trait]
impl LiveChecker for HttpLiveChecker {
    async fn telegram_get_me(&self, token: &str) -> Result<BotInfo, LiveCheckError> {
        // The token is in the path. Never surface the reqwest error: it includes the URL.
        let url = format!("{}/bot{token}/getMe", self.telegram_api_base);
        let response = self.client.get(url).send().await.map_err(|_| {
            LiveCheckError::Unreachable(
                "Telegram did not respond. Check your network and try again.".into(),
            )
        })?;

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(LiveCheckError::Rejected(
                "Telegram rejected this token. Check it in @BotFather.".into(),
            ));
        }
        if !status.is_success() {
            return Err(LiveCheckError::Unreachable(format!(
                "Telegram returned HTTP {status}."
            )));
        }

        let body: TelegramResponse = response.json().await.map_err(|_| {
            LiveCheckError::Unreachable("Telegram returned a response that was not JSON.".into())
        })?;

        if !body.ok {
            let reason = body
                .description
                .unwrap_or_else(|| "Telegram rejected this token.".into());
            return Err(LiveCheckError::Rejected(reason));
        }

        let user = body.result.ok_or_else(|| {
            LiveCheckError::Rejected("Telegram did not return a bot identity.".into())
        })?;
        let username = user
            .username
            .or(user.first_name)
            .unwrap_or_else(|| "bot".into());

        Ok(BotInfo {
            id: user.id,
            username,
        })
    }

    async fn ping_coingecko(
        &self,
        base_url: &str,
        api_key: Option<&str>,
    ) -> Result<(), LiveCheckError> {
        let base = base_url.trim_end_matches('/');
        let mut request = self.client.get(format!("{base}/ping"));
        if let Some(key) = api_key {
            let header = if base.contains("pro-api.coingecko.com") {
                "x-cg-pro-api-key"
            } else {
                "x-cg-demo-api-key"
            };
            request = request.header(header, key);
        }

        let response = request.send().await.map_err(|_| {
            LiveCheckError::Unreachable(
                "CoinGecko did not respond. Check your network, or skip the key for now.".into(),
            )
        })?;

        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(LiveCheckError::Rejected(
                "CoinGecko rejected this API key.".into(),
            ));
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(LiveCheckError::Unreachable(
                "CoinGecko rate-limited this request. A key helps; you can skip for now.".into(),
            ));
        }
        if !status.is_success() {
            return Err(LiveCheckError::Unreachable(format!(
                "CoinGecko returned HTTP {status}."
            )));
        }
        Ok(())
    }

    async fn ping_solana_rpc(&self, url: &str) -> Result<(), LiveCheckError> {
        let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getSlot"});
        let response = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|_| {
                LiveCheckError::Unreachable(
                    "The RPC endpoint did not respond. Check the URL and your network.".into(),
                )
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(LiveCheckError::Unreachable(format!(
                "The RPC endpoint returned HTTP {status}."
            )));
        }

        let parsed: RpcResponse = response.json().await.map_err(|_| {
            LiveCheckError::Unreachable("The RPC endpoint returned a non-JSON body.".into())
        })?;

        if let Some(error) = parsed.error {
            return Err(LiveCheckError::Rejected(format!(
                "RPC error: {}",
                error.message
            )));
        }
        if parsed.result.is_none() {
            return Err(LiveCheckError::Rejected(
                "The RPC endpoint did not return a slot.".into(),
            ));
        }
        Ok(())
    }
}
