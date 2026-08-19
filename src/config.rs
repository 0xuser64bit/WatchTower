use crate::error::{AppError, Result};
use config::{Config, Environment};

#[derive(Debug, Clone)]
pub struct Settings {
    pub telegram_bot_token: String,
    pub admin_telegram_ids: Vec<i64>,
    pub database_url: String,
    pub coingecko_api_url: String,
    pub price_fallback_urls: Vec<String>,
    pub solana_rpc_endpoints: Vec<String>,
    pub solana_rpc_commitment: String,
    pub poll_interval_seconds: u64,
    pub alert_default_cooldown_seconds: i64,
    pub log_dir: String,
    pub log_max_files: usize,
}

impl Settings {
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();

        let builder = Config::builder()
            .set_default("COINGECKO_API_URL", "https://api.coingecko.com/api/v3")?
            .set_default("SOLANA_RPC_COMMITMENT", "confirmed")?
            .set_default("POLL_INTERVAL_SECONDS", 60)?
            .set_default("ALERT_DEFAULT_COOLDOWN_SECONDS", 300)?
            .set_default("LOG_DIR", "logs")?
            .set_default("LOG_MAX_FILES", 14)?;

        let cfg = builder
            .add_source(Environment::default().separator("__"))
            .build()?;

        let settings = Settings {
            telegram_bot_token: cfg.get_string("TELEGRAM_BOT_TOKEN")?,
            admin_telegram_ids: parse_ids(&cfg.get_string("ADMIN_TELEGRAM_IDS")?)?,
            database_url: cfg.get_string("DATABASE_URL")?,
            coingecko_api_url: cfg.get_string("COINGECKO_API_URL")?,
            price_fallback_urls: parse_list(
                &cfg.get_string("PRICE_FALLBACK_URLS").unwrap_or_default(),
            ),
            solana_rpc_endpoints: parse_list(&cfg.get_string("SOLANA_RPC_ENDPOINTS")?),
            solana_rpc_commitment: cfg.get_string("SOLANA_RPC_COMMITMENT")?,
            poll_interval_seconds: cfg.get::<u64>("POLL_INTERVAL_SECONDS")?,
            alert_default_cooldown_seconds: cfg.get::<i64>("ALERT_DEFAULT_COOLDOWN_SECONDS")?,
            log_dir: cfg.get_string("LOG_DIR")?,
            log_max_files: cfg.get::<usize>("LOG_MAX_FILES")?,
        };

        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<()> {
        if self.telegram_bot_token.is_empty() || self.telegram_bot_token == "replace_me" {
            return Err(AppError::Config(config::ConfigError::Message(
                "TELEGRAM_BOT_TOKEN is not set".into(),
            )));
        }

        if self.admin_telegram_ids.is_empty() {
            return Err(AppError::Config(config::ConfigError::Message(
                "ADMIN_TELEGRAM_IDS must contain at least one user id".into(),
            )));
        }

        if self.solana_rpc_endpoints.is_empty() {
            return Err(AppError::Config(config::ConfigError::Message(
                "SOLANA_RPC_ENDPOINTS must contain at least one endpoint".into(),
            )));
        }

        if self.poll_interval_seconds == 0 {
            return Err(AppError::Config(config::ConfigError::Message(
                "POLL_INTERVAL_SECONDS must be greater than zero".into(),
            )));
        }

        if !matches!(
            self.solana_rpc_commitment.as_str(),
            "processed" | "confirmed" | "finalized"
        ) {
            return Err(AppError::Config(config::ConfigError::Message(
                "SOLANA_RPC_COMMITMENT must be processed, confirmed, or finalized".into(),
            )));
        }

        if self.alert_default_cooldown_seconds < 0 {
            return Err(AppError::Config(config::ConfigError::Message(
                "ALERT_DEFAULT_COOLDOWN_SECONDS must be zero or greater".into(),
            )));
        }

        if self.log_dir.trim().is_empty() {
            return Err(AppError::Config(config::ConfigError::Message(
                "LOG_DIR must not be empty".into(),
            )));
        }

        Ok(())
    }
}

fn parse_ids(raw: &str) -> Result<Vec<i64>> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<i64>().map_err(|_| {
                AppError::Config(config::ConfigError::Message(format!(
                    "invalid telegram id: {s}"
                )))
            })
        })
        .collect()
}

fn parse_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_separated_ids() {
        let ids = parse_ids("123, 456 , 789").unwrap();
        assert_eq!(ids, vec![123, 456, 789]);
    }

    #[test]
    fn rejects_invalid_id() {
        assert!(parse_ids("123,abc").is_err());
    }

    #[test]
    fn parses_empty_list() {
        assert!(parse_list("").is_empty());
        assert_eq!(parse_list("a, b, c"), vec!["a", "b", "c"]);
    }

    fn valid_settings() -> Settings {
        Settings {
            telegram_bot_token: "token".into(),
            admin_telegram_ids: vec![123],
            database_url: "sqlite::memory:".into(),
            coingecko_api_url: "https://api.coingecko.com/api/v3".into(),
            price_fallback_urls: vec![],
            solana_rpc_endpoints: vec!["https://api.mainnet-beta.solana.com".into()],
            solana_rpc_commitment: "confirmed".into(),
            poll_interval_seconds: 60,
            alert_default_cooldown_seconds: 300,
            log_dir: "logs".into(),
            log_max_files: 14,
        }
    }

    #[test]
    fn rejects_bad_commitment() {
        let mut settings = valid_settings();
        settings.solana_rpc_commitment = "unknown".into();
        assert!(settings.validate().is_err());
    }

    #[test]
    fn rejects_negative_cooldown() {
        let mut settings = valid_settings();
        settings.alert_default_cooldown_seconds = -1;
        assert!(settings.validate().is_err());
    }
}
