//! Typed, validated runtime configuration.
//!
//! Configuration comes exclusively from the process environment (optionally seeded
//! from a `.env` file). Parsing is pure and testable: [`Settings::from_env_map`]
//! takes a plain map so the whole surface can be exercised without mutating global
//! process state.
//!
//! Operator-facing copy, defaults, and wizard metadata live in [`fields`]. The
//! parser reads defaults from that catalog so the two cannot drift.

mod fields;

pub use fields::{FieldSpec, FieldTier, FIELD_CATALOG};

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

/// Lowest poll interval we accept. Anything faster gets a public RPC / price API
/// to rate-limit or ban the host, which silently breaks alerting.
const MIN_POLL_INTERVAL_SECONDS: u64 = 10;
const MAX_POLL_INTERVAL_SECONDS: u64 = 86_400;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("{key} is required but not set")]
    Missing { key: &'static str },
    #[error("{key} is invalid: {reason}")]
    Invalid { key: &'static str, reason: String },
}

type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commitment {
    Processed,
    Confirmed,
    Finalized,
}

impl Commitment {
    pub fn as_str(self) -> &'static str {
        match self {
            Commitment::Processed => "processed",
            Commitment::Confirmed => "confirmed",
            Commitment::Finalized => "finalized",
        }
    }
}

/// A string that never renders its contents through `Debug`/`Display`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub telegram_bot_token: Secret,
    /// Bootstrap admins. Seeded into the database on startup; the database is the
    /// authority afterwards, so removing an id here does not revoke access.
    pub admin_telegram_ids: Vec<i64>,
    pub database_url: String,
    /// CoinGecko-compatible base URLs. The first is primary, the rest are tried in
    /// order when the primary fails.
    pub coingecko_api_urls: Vec<String>,
    pub coingecko_api_key: Option<Secret>,
    pub solana_rpc_endpoints: Vec<String>,
    pub solana_rpc_commitment: Commitment,
    pub poll_interval: Duration,
    pub http_timeout: Duration,
    pub alert_default_cooldown_seconds: i64,
    pub alert_history_retention_days: i64,
    pub log_dir: String,
    pub log_max_files: usize,
}

impl Settings {
    /// Loads `.env` (if present) and parses the process environment.
    pub fn load() -> Result<Self> {
        let _ = dotenvy::dotenv();
        Self::from_env_map(&std::env::vars().collect())
    }

    pub fn from_env_map(env: &HashMap<String, String>) -> Result<Self> {
        let settings = Settings {
            telegram_bot_token: Secret(required(env, "TELEGRAM_BOT_TOKEN")?),
            admin_telegram_ids: parse_admin_ids(&required(env, "ADMIN_TELEGRAM_IDS")?)?,
            database_url: optional(env, "DATABASE_URL")
                .unwrap_or_else(|| FieldSpec::default_value("DATABASE_URL").to_string()),
            coingecko_api_urls: parse_urls(
                "COINGECKO_API_URLS",
                optional(env, "COINGECKO_API_URLS")
                    .as_deref()
                    .unwrap_or_else(|| FieldSpec::default_value("COINGECKO_API_URLS")),
            )?,
            coingecko_api_key: optional(env, "COINGECKO_API_KEY").map(Secret),
            solana_rpc_endpoints: parse_urls(
                "SOLANA_RPC_ENDPOINTS",
                optional(env, "SOLANA_RPC_ENDPOINTS")
                    .as_deref()
                    .unwrap_or_else(|| FieldSpec::default_value("SOLANA_RPC_ENDPOINTS")),
            )?,
            solana_rpc_commitment: parse_commitment(
                optional(env, "SOLANA_RPC_COMMITMENT")
                    .as_deref()
                    .unwrap_or_else(|| FieldSpec::default_value("SOLANA_RPC_COMMITMENT")),
            )?,
            poll_interval: Duration::from_secs(parse_num(
                env,
                "POLL_INTERVAL_SECONDS",
                catalog_u64("POLL_INTERVAL_SECONDS"),
                MIN_POLL_INTERVAL_SECONDS,
                MAX_POLL_INTERVAL_SECONDS,
            )?),
            http_timeout: Duration::from_secs(parse_num(
                env,
                "HTTP_TIMEOUT_SECONDS",
                catalog_u64("HTTP_TIMEOUT_SECONDS"),
                1,
                120,
            )?),
            alert_default_cooldown_seconds: parse_num(
                env,
                "ALERT_DEFAULT_COOLDOWN_SECONDS",
                catalog_i64("ALERT_DEFAULT_COOLDOWN_SECONDS"),
                0,
                86_400,
            )?,
            alert_history_retention_days: parse_num(
                env,
                "ALERT_HISTORY_RETENTION_DAYS",
                catalog_i64("ALERT_HISTORY_RETENTION_DAYS"),
                1,
                3_650,
            )?,
            log_dir: optional(env, "LOG_DIR")
                .unwrap_or_else(|| FieldSpec::default_value("LOG_DIR").to_string()),
            log_max_files: parse_num(env, "LOG_MAX_FILES", catalog_u64("LOG_MAX_FILES"), 1, 365)?
                as usize,
        };

        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<()> {
        // Reject placeholders and obvious typos before the process starts long-polling
        // with a token Telegram will reject.
        validate_telegram_bot_token(self.telegram_bot_token.expose())?;

        if !self.database_url.starts_with("sqlite:") {
            return Err(ConfigError::Invalid {
                key: "DATABASE_URL",
                reason: "only sqlite: URLs are supported".into(),
            });
        }

        if self.log_dir.trim().is_empty() {
            return Err(ConfigError::Invalid {
                key: "LOG_DIR",
                reason: "must not be empty".into(),
            });
        }

        Ok(())
    }
}

fn catalog_u64(key: &str) -> u64 {
    FieldSpec::default_value(key)
        .parse()
        .unwrap_or_else(|_| panic!("config catalog: {key} default is not a u64"))
}

fn catalog_i64(key: &str) -> i64 {
    FieldSpec::default_value(key)
        .parse()
        .unwrap_or_else(|_| panic!("config catalog: {key} default is not an i64"))
}

pub(crate) fn validate_telegram_bot_token(token: &str) -> Result<()> {
    if token == "replace_me" || !token.contains(':') || token.len() < 20 {
        return Err(ConfigError::Invalid {
            key: "TELEGRAM_BOT_TOKEN",
            reason: "expected a token in the form <bot_id>:<secret> from @BotFather".into(),
        });
    }
    Ok(())
}

fn optional(env: &HashMap<String, String>, key: &str) -> Option<String> {
    env.get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn required(env: &HashMap<String, String>, key: &'static str) -> Result<String> {
    optional(env, key).ok_or(ConfigError::Missing { key })
}

fn parse_num<T>(
    env: &HashMap<String, String>,
    key: &'static str,
    default: T,
    min: T,
    max: T,
) -> Result<T>
where
    T: std::str::FromStr + PartialOrd + fmt::Display + Copy,
{
    let Some(raw) = optional(env, key) else {
        return Ok(default);
    };

    let value = raw.parse::<T>().map_err(|_| ConfigError::Invalid {
        key,
        reason: format!("`{raw}` is not a valid number"),
    })?;

    if value < min || value > max {
        return Err(ConfigError::Invalid {
            key,
            reason: format!("must be between {min} and {max} (got {value})"),
        });
    }

    Ok(value)
}

pub(crate) fn parse_admin_ids(raw: &str) -> Result<Vec<i64>> {
    let mut ids = Vec::new();

    for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let id = part.parse::<i64>().map_err(|_| ConfigError::Invalid {
            key: "ADMIN_TELEGRAM_IDS",
            reason: format!("`{part}` is not a numeric Telegram user id"),
        })?;

        if id <= 0 {
            return Err(ConfigError::Invalid {
                key: "ADMIN_TELEGRAM_IDS",
                reason: format!("`{part}` must be a positive Telegram user id"),
            });
        }

        if !ids.contains(&id) {
            ids.push(id);
        }
    }

    if ids.is_empty() {
        return Err(ConfigError::Invalid {
            key: "ADMIN_TELEGRAM_IDS",
            reason: "at least one Telegram user id is required".into(),
        });
    }

    Ok(ids)
}

pub(crate) fn parse_urls(key: &'static str, raw: &str) -> Result<Vec<String>> {
    let mut urls = Vec::new();

    for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if !part.starts_with("http://") && !part.starts_with("https://") {
            return Err(ConfigError::Invalid {
                key,
                reason: format!("`{part}` must start with http:// or https://"),
            });
        }

        let normalized = part.trim_end_matches('/').to_string();
        if !urls.contains(&normalized) {
            urls.push(normalized);
        }
    }

    if urls.is_empty() {
        return Err(ConfigError::Invalid {
            key,
            reason: "at least one URL is required".into(),
        });
    }

    Ok(urls)
}

pub(crate) fn parse_commitment(raw: &str) -> Result<Commitment> {
    match raw {
        "processed" => Ok(Commitment::Processed),
        "confirmed" => Ok(Commitment::Confirmed),
        "finalized" => Ok(Commitment::Finalized),
        other => Err(ConfigError::Invalid {
            key: "SOLANA_RPC_COMMITMENT",
            reason: format!("`{other}` must be processed, confirmed, or finalized"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> HashMap<String, String> {
        HashMap::from([
            (
                "TELEGRAM_BOT_TOKEN".to_string(),
                "1234567890:AAEhBOweik6ad".to_string(),
            ),
            ("ADMIN_TELEGRAM_IDS".to_string(), "111,222".to_string()),
        ])
    }

    fn with(key: &str, value: &str) -> HashMap<String, String> {
        let mut env = base();
        env.insert(key.to_string(), value.to_string());
        env
    }

    #[test]
    fn applies_documented_defaults() {
        let settings = Settings::from_env_map(&base()).unwrap();
        assert_eq!(settings.poll_interval, Duration::from_secs(60));
        assert_eq!(settings.solana_rpc_commitment, Commitment::Confirmed);
        assert_eq!(settings.alert_default_cooldown_seconds, 300);
        assert_eq!(settings.database_url, "sqlite://data/watchtower.db");
        assert_eq!(
            settings.coingecko_api_urls,
            vec!["https://api.coingecko.com/api/v3"]
        );
    }

    #[test]
    fn requires_bot_token_and_admins() {
        let mut env = base();
        env.remove("TELEGRAM_BOT_TOKEN");
        assert!(matches!(
            Settings::from_env_map(&env),
            Err(ConfigError::Missing {
                key: "TELEGRAM_BOT_TOKEN"
            })
        ));

        let mut env = base();
        env.remove("ADMIN_TELEGRAM_IDS");
        assert!(matches!(
            Settings::from_env_map(&env),
            Err(ConfigError::Missing {
                key: "ADMIN_TELEGRAM_IDS"
            })
        ));
    }

    #[test]
    fn rejects_placeholder_token() {
        assert!(Settings::from_env_map(&with("TELEGRAM_BOT_TOKEN", "replace_me")).is_err());
        assert!(Settings::from_env_map(&with("TELEGRAM_BOT_TOKEN", "nocolon")).is_err());
    }

    #[test]
    fn deduplicates_and_validates_admin_ids() {
        let settings = Settings::from_env_map(&with("ADMIN_TELEGRAM_IDS", "5, 5 ,7")).unwrap();
        assert_eq!(settings.admin_telegram_ids, vec![5, 7]);

        assert!(Settings::from_env_map(&with("ADMIN_TELEGRAM_IDS", "abc")).is_err());
        assert!(Settings::from_env_map(&with("ADMIN_TELEGRAM_IDS", "-1")).is_err());
        assert!(Settings::from_env_map(&with("ADMIN_TELEGRAM_IDS", " , ")).is_err());
    }

    #[test]
    fn enforces_poll_interval_floor() {
        assert!(Settings::from_env_map(&with("POLL_INTERVAL_SECONDS", "1")).is_err());
        assert!(Settings::from_env_map(&with("POLL_INTERVAL_SECONDS", "0")).is_err());
        let settings = Settings::from_env_map(&with("POLL_INTERVAL_SECONDS", "10")).unwrap();
        assert_eq!(settings.poll_interval, Duration::from_secs(10));
    }

    #[test]
    fn rejects_non_http_urls_and_bad_commitment() {
        assert!(Settings::from_env_map(&with("SOLANA_RPC_ENDPOINTS", "ftp://x")).is_err());
        assert!(Settings::from_env_map(&with("SOLANA_RPC_COMMITMENT", "eventual")).is_err());
        assert!(Settings::from_env_map(&with("DATABASE_URL", "postgres://x")).is_err());
    }

    #[test]
    fn normalizes_and_deduplicates_urls() {
        let settings = Settings::from_env_map(&with(
            "SOLANA_RPC_ENDPOINTS",
            "https://a.example/ , https://a.example, https://b.example",
        ))
        .unwrap();
        assert_eq!(
            settings.solana_rpc_endpoints,
            vec!["https://a.example", "https://b.example"]
        );
    }

    #[test]
    fn never_renders_secrets() {
        let settings = Settings::from_env_map(&base()).unwrap();
        let rendered = format!("{settings:?}");
        assert!(!rendered.contains("AAEhBOweik6ad"), "{rendered}");
        assert!(rendered.contains("<redacted>"));
    }
}
