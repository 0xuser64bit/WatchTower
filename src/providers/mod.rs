//! External data providers.
//!
//! Two narrow traits keep the engine independent of any particular vendor and make
//! the scheduler testable without network access.

pub mod price;
pub mod solana;

use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;
use tracing::warn;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// The provider answered, but not with something we can use. Deterministic:
    /// retrying the same request will produce the same result.
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    /// The provider is temporarily refusing or failing. Worth retrying.
    #[error("provider unavailable: {0}")]
    Unavailable(String),

    #[error("rate limited by provider{}", match .retry_after {
        Some(d) => format!(", retry after {}s", d.as_secs()),
        None => String::new(),
    })]
    RateLimited { retry_after: Option<Duration> },

    /// The provider does not know about this asset. Retrying will not help, and the
    /// distinction matters: it means the user's rule can never fire.
    #[error("not supported by provider: {0}")]
    Unsupported(String),
}

impl ProviderError {
    /// Whether another attempt could plausibly succeed.
    pub fn is_retryable(&self) -> bool {
        match self {
            ProviderError::Http(err) => {
                err.is_timeout() || err.is_connect() || err.is_request() || err.is_body()
            }
            ProviderError::Unavailable(_) | ProviderError::RateLimited { .. } => true,
            ProviderError::InvalidResponse(_) | ProviderError::Unsupported(_) => false,
        }
    }
}

pub type ProviderResult<T> = Result<T, ProviderError>;

/// Retries `operation` with exponential backoff while the error is retryable.
/// Public APIs used here rate-limit aggressively, so transient failures should not
/// end a poll before the retry budget is exhausted.
pub async fn with_retry<T, F, Fut>(
    label: &'static str,
    attempts: u32,
    base_delay: Duration,
    mut operation: F,
) -> ProviderResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ProviderResult<T>>,
{
    debug_assert!(attempts >= 1, "at least one attempt is required");

    let mut last_error = None;

    for attempt in 1..=attempts.max(1) {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if !err.is_retryable() || attempt == attempts {
                    return Err(err);
                }

                // Honour a server-provided backoff when there is one; otherwise
                // double the delay each attempt.
                let delay = match &err {
                    ProviderError::RateLimited {
                        retry_after: Some(after),
                    } => (*after).min(Duration::from_secs(30)),
                    _ => base_delay * 2u32.pow(attempt - 1),
                };

                warn!(
                    provider = label,
                    attempt,
                    delay_ms = delay.as_millis(),
                    %err,
                    "provider request failed, retrying"
                );

                tokio::time::sleep(delay).await;
                last_error = Some(err);
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| ProviderError::Unavailable(format!("{label} exhausted all attempts"))))
}

#[async_trait]
pub trait PriceProvider: Send + Sync {
    /// USD price of one unit of the SPL token identified by `mint`.
    async fn get_token_price_usd(&self, mint: &str) -> ProviderResult<f64>;
}

#[async_trait]
pub trait ChainProvider: Send + Sync {
    /// Native SOL balance of a single address, in lamports.
    async fn get_native_balance_lamports(&self, address: &str) -> ProviderResult<u64>;

    /// Native balances for many addresses, in the same order as the input.
    ///
    /// The scheduler uses this so that watching N wallets costs a bounded number of
    /// requests per tick instead of N.
    async fn get_native_balances_lamports(&self, addresses: &[String]) -> ProviderResult<Vec<u64>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test(start_paused = true)]
    async fn retries_until_success() {
        let calls = AtomicU32::new(0);

        let result = with_retry("test", 5, Duration::from_millis(10), || async {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err(ProviderError::Unavailable("boom".into()))
            } else {
                Ok(42)
            }
        })
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_retry_deterministic_failures() {
        let calls = AtomicU32::new(0);

        let result: ProviderResult<()> =
            with_retry("test", 5, Duration::from_millis(10), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(ProviderError::InvalidResponse("malformed".into()))
            })
            .await;

        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "must not retry");
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_the_attempt_budget() {
        let calls = AtomicU32::new(0);

        let result: ProviderResult<()> =
            with_retry("test", 3, Duration::from_millis(1), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err(ProviderError::Unavailable("still down".into()))
            })
            .await;

        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn unsupported_assets_are_not_retryable() {
        assert!(!ProviderError::Unsupported("unknown mint".into()).is_retryable());
        assert!(ProviderError::RateLimited { retry_after: None }.is_retryable());
    }
}
