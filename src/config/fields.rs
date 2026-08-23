//! Operator-facing catalog of every environment variable WatchTower understands.
//!
//! The daemon parser, the setup wizard, and the generated `.env` comments all
//! read this table so copy, defaults, and validation cannot drift.

/// Whether the wizard always asks, recommends, or hides the field behind Advanced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldTier {
    Required,
    Recommended,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldSpec {
    pub key: &'static str,
    pub tier: FieldTier,
    /// Documented default when the variable is unset. `None` for required values
    /// and for optional secrets that mean "not set".
    pub default: Option<&'static str>,
    pub secret: bool,
    /// One-line recap / table label.
    pub summary: &'static str,
    /// Multi-paragraph explanation shown in the wizard and written as `.env` comments.
    pub explanation: &'static str,
    pub how_to_get: Option<&'static str>,
    pub constraints: &'static str,
}

impl FieldSpec {
    pub fn get(key: &str) -> Option<&'static FieldSpec> {
        FIELD_CATALOG.iter().find(|field| field.key == key)
    }

    pub fn default_value(key: &str) -> &'static str {
        Self::get(key)
            .and_then(|field| field.default)
            .unwrap_or_else(|| panic!("config catalog: {key} has no default"))
    }

    pub fn title(&self) -> String {
        let tier = match self.tier {
            FieldTier::Required => "required",
            FieldTier::Recommended => "recommended",
            FieldTier::Advanced => "advanced",
        };
        format!("{} ({tier})", self.key)
    }
}

pub const FIELD_CATALOG: &[FieldSpec] = &[
    FieldSpec {
        key: "TELEGRAM_BOT_TOKEN",
        tier: FieldTier::Required,
        default: None,
        secret: true,
        summary: "Bot credential from @BotFather. Anyone who has it controls the bot.",
        explanation: "\
WatchTower is controlled exclusively through Telegram. This token is the bot's
credential: the daemon uses it to long-poll for commands and to send alerts.
Anyone who has the token can impersonate the bot, so treat it like a password
and rotate it in @BotFather if it leaks.

The daemon will not start with a placeholder or a string that is not a Telegram
token. There is no anonymous mode.",
        how_to_get: Some(
            "\
  1. Open https://t.me/BotFather (or search @BotFather in Telegram).
  2. Send /newbot, choose a name and a username ending in `bot`.
  3. Copy the token BotFather prints. It looks like 123456789:AA.....",
        ),
        constraints: "Form <bot_id>:<secret>, at least 20 characters. Input is hidden.",
    },
    FieldSpec {
        key: "ADMIN_TELEGRAM_IDS",
        tier: FieldTier::Required,
        default: None,
        secret: false,
        summary: "Numeric Telegram user ids seeded as administrators on first start.",
        explanation: "\
These numeric user ids become administrators the first time the daemon starts.
Usernames are not identity — they can be changed or spoofed — so only the
number is accepted.

The ids SEED the users table. After that the database is the authority:
removing an id from this variable does not demote that person, and an admin
demoted through /admin is not re-promoted on restart. Leave this empty and
the daemon will refuse to start; a placeholder would grant a stranger control.",
        how_to_get: Some(
            "\
  1. Open https://t.me/userinfobot (or search @userinfobot).
  2. Send any message; copy the Id number it replies with.
  3. Repeat for every person who should be an administrator.",
        ),
        constraints: "Comma-separated positive integers. At least one id is required.",
    },
    FieldSpec {
        key: "COINGECKO_API_KEY",
        tier: FieldTier::Recommended,
        default: None,
        secret: true,
        summary: "Optional CoinGecko key. Avoids public-tier rate limits on price polls.",
        explanation: "\
Each poll makes one CoinGecko request per tracked token. The unauthenticated
public tier rate-limits quickly; when that happens prices look frozen and
alerts do not fire. A free demo key is enough for personal use.

The key is sent as `x-cg-demo-api-key`, or `x-cg-pro-api-key` when the API
URL points at pro-api.coingecko.com (set under Advanced).",
        how_to_get: Some(
            "\
  Create a demo key at:
  https://www.coingecko.com/en/developers/dashboard",
        ),
        constraints: "Optional. Leave unset to use the public tier (fine for a quick test).",
    },
    FieldSpec {
        key: "SOLANA_RPC_ENDPOINTS",
        tier: FieldTier::Recommended,
        default: Some("https://api.mainnet-beta.solana.com"),
        secret: false,
        summary: "JSON-RPC URLs for native SOL balances, with ordered failover.",
        explanation: "\
Wallet monitoring reads native SOL balances in one batched RPC call per poll.
The public mainnet endpoint is the default and is heavily rate-limited — fine
for a throwaway test, not for anything you rely on.

Give at least two URLs in production. An endpoint that fails is benched for
60 seconds and traffic moves to the next one.",
        how_to_get: Some(
            "\
  A private endpoint from Helius, Triton, QuickNode, or your own validator.
  Paste the HTTPS URL, then add a failover if you have one.",
        ),
        constraints: "Comma-separated http:// or https:// URLs. At least one is required.",
    },
    FieldSpec {
        key: "DATABASE_URL",
        tier: FieldTier::Advanced,
        default: Some("sqlite://data/watchtower.db"),
        secret: false,
        summary: "SQLite database URL. Parent directory is created automatically.",
        explanation: "\
SQLite is the only supported backend. It holds users, tracked tokens and
wallets, alert rules, and alert history. The parent directory is created on
startup. Back this file up together with .env; reset (./scripts/ctl.sh reset)
deletes it.",
        how_to_get: None,
        constraints: "Must start with sqlite:. Default sqlite://data/watchtower.db",
    },
    FieldSpec {
        key: "COINGECKO_API_URLS",
        tier: FieldTier::Advanced,
        default: Some("https://api.coingecko.com/api/v3"),
        secret: false,
        summary: "CoinGecko-compatible API roots, ordered failover.",
        explanation: "\
Ordered list of CoinGecko-compatible API roots. The first is primary; the
rest are tried in order when it fails. Point this at pro-api.coingecko.com
when using a paid key.",
        how_to_get: None,
        constraints: "Comma-separated http:// or https:// URLs.",
    },
    FieldSpec {
        key: "SOLANA_RPC_COMMITMENT",
        tier: FieldTier::Advanced,
        default: Some("confirmed"),
        secret: false,
        summary: "Solana commitment level used for balance reads.",
        explanation: "\
How finalized a slot must be before a balance is trusted. `processed` is
fastest and can roll back, `finalized` is slowest and irreversible.
`confirmed` is the usual compromise for alerting.",
        how_to_get: None,
        constraints: "processed | confirmed | finalized",
    },
    FieldSpec {
        key: "HTTP_TIMEOUT_SECONDS",
        tier: FieldTier::Advanced,
        default: Some("10"),
        secret: false,
        summary: "Per-request HTTP timeout for Telegram, CoinGecko, and RPC.",
        explanation: "\
Per-request timeout applied to CoinGecko, Solana RPC, and setup live checks.
Too low and a slow RPC looks down; too high and a hung provider stalls a poll.",
        how_to_get: None,
        constraints: "Integer seconds, 1–120. Default 10.",
    },
    FieldSpec {
        key: "POLL_INTERVAL_SECONDS",
        tier: FieldTier::Advanced,
        default: Some("60"),
        secret: false,
        summary: "Seconds between monitor polls. Lower values cost more API traffic.",
        explanation: "\
Seconds between monitor polls. Each poll costs one CoinGecko request per
tracked token plus one batched RPC call for all wallets, so values much
below 60 need an API key and a private RPC endpoint. This is interval
monitoring, not mempool or transaction watching.",
        how_to_get: None,
        constraints: "Integer seconds, 10–86400. Default 60.",
    },
    FieldSpec {
        key: "ALERT_DEFAULT_COOLDOWN_SECONDS",
        tier: FieldTier::Advanced,
        default: Some("300"),
        secret: false,
        summary: "Default cooldown applied to new rules created in /addalert.",
        explanation: "\
Default minimum seconds between repeat alerts for a NEW rule. Each rule
stores its own value, chosen during /addalert, so changing this does not
rewrite existing rules.

Alerts are edge-triggered: a rule fires when its condition becomes true and
stays quiet until the condition clears. Cooldown only limits a condition
that keeps flipping across the threshold.",
        how_to_get: None,
        constraints: "Integer seconds, 0–86400. Default 300.",
    },
    FieldSpec {
        key: "ALERT_HISTORY_RETENTION_DAYS",
        tier: FieldTier::Advanced,
        default: Some("90"),
        secret: false,
        summary: "How long alert history rows are kept before pruning.",
        explanation: "\
Days of alert history to keep. Pruned every 6 hours. History is a snapshot,
so it remains readable after a rule or target is deleted.",
        how_to_get: None,
        constraints: "Integer days, 1–3650. Default 90.",
    },
    FieldSpec {
        key: "LOG_DIR",
        tier: FieldTier::Advanced,
        default: Some("logs"),
        secret: false,
        summary: "Directory for daily rolling file logs (stdout is also always used).",
        explanation: "\
Logs go to stdout and to a daily rolling file in this directory. Under
systemd, stdout (journalctl) is the authoritative stream; the files are a
convenience for local runs.",
        how_to_get: None,
        constraints: "Non-empty path. Default logs",
    },
    FieldSpec {
        key: "LOG_MAX_FILES",
        tier: FieldTier::Advanced,
        default: Some("14"),
        secret: false,
        summary: "How many daily log files to retain.",
        explanation: "\
Number of daily rolling log files to keep in LOG_DIR before the oldest is
deleted.",
        how_to_get: None,
        constraints: "Integer, 1–365. Default 14.",
    },
    FieldSpec {
        key: "RUST_LOG",
        tier: FieldTier::Advanced,
        default: Some("info,watchtower=info"),
        secret: false,
        summary: "tracing-subscriber filter. Not required for the daemon to start.",
        explanation: "\
Env-filter for tracing-subscriber. Increase to `debug` when chasing provider
or Telegram issues. This variable is read by the logging stack, not by
Settings, and is safe to leave unset.",
        how_to_get: None,
        constraints: "A tracing-subscriber EnvFilter string.",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;
    use std::collections::{HashMap, HashSet};

    fn base() -> HashMap<String, String> {
        HashMap::from([
            (
                "TELEGRAM_BOT_TOKEN".to_string(),
                "1234567890:AAEhBOweik6ad".to_string(),
            ),
            ("ADMIN_TELEGRAM_IDS".to_string(), "111,222".to_string()),
        ])
    }

    #[test]
    fn catalog_keys_are_unique_and_cover_settings() {
        let mut seen = HashSet::new();
        for field in FIELD_CATALOG {
            assert!(
                seen.insert(field.key),
                "duplicate catalog key {}",
                field.key
            );
        }

        for key in [
            "TELEGRAM_BOT_TOKEN",
            "ADMIN_TELEGRAM_IDS",
            "DATABASE_URL",
            "COINGECKO_API_URLS",
            "COINGECKO_API_KEY",
            "SOLANA_RPC_ENDPOINTS",
            "SOLANA_RPC_COMMITMENT",
            "POLL_INTERVAL_SECONDS",
            "HTTP_TIMEOUT_SECONDS",
            "ALERT_DEFAULT_COOLDOWN_SECONDS",
            "ALERT_HISTORY_RETENTION_DAYS",
            "LOG_DIR",
            "LOG_MAX_FILES",
            "RUST_LOG",
        ] {
            assert!(FieldSpec::get(key).is_some(), "{key} missing from catalog");
        }

        assert_eq!(
            FieldSpec::get("TELEGRAM_BOT_TOKEN").unwrap().tier,
            FieldTier::Required
        );
        assert_eq!(
            FieldSpec::get("ADMIN_TELEGRAM_IDS").unwrap().tier,
            FieldTier::Required
        );
        assert_eq!(
            FieldSpec::get("COINGECKO_API_KEY").unwrap().tier,
            FieldTier::Recommended
        );
        assert_eq!(
            FieldSpec::get("SOLANA_RPC_ENDPOINTS").unwrap().tier,
            FieldTier::Recommended
        );
    }

    #[test]
    fn required_fields_have_no_default() {
        for field in FIELD_CATALOG {
            if field.tier == FieldTier::Required {
                assert!(
                    field.default.is_none(),
                    "{} is required and must not have a default",
                    field.key
                );
            }
        }
    }

    #[test]
    fn catalog_defaults_match_settings() {
        let settings = Settings::from_env_map(&base()).unwrap();
        assert_eq!(
            settings.database_url,
            FieldSpec::default_value("DATABASE_URL")
        );
        assert_eq!(
            settings.coingecko_api_urls,
            vec![FieldSpec::default_value("COINGECKO_API_URLS").to_string()]
        );
        assert_eq!(
            settings.solana_rpc_endpoints,
            vec![FieldSpec::default_value("SOLANA_RPC_ENDPOINTS").to_string()]
        );
        assert_eq!(
            settings.solana_rpc_commitment.as_str(),
            FieldSpec::default_value("SOLANA_RPC_COMMITMENT")
        );
        assert_eq!(
            settings.poll_interval.as_secs().to_string(),
            FieldSpec::default_value("POLL_INTERVAL_SECONDS")
        );
        assert_eq!(
            settings.http_timeout.as_secs().to_string(),
            FieldSpec::default_value("HTTP_TIMEOUT_SECONDS")
        );
        assert_eq!(
            settings.alert_default_cooldown_seconds.to_string(),
            FieldSpec::default_value("ALERT_DEFAULT_COOLDOWN_SECONDS")
        );
        assert_eq!(
            settings.alert_history_retention_days.to_string(),
            FieldSpec::default_value("ALERT_HISTORY_RETENTION_DAYS")
        );
        assert_eq!(settings.log_dir, FieldSpec::default_value("LOG_DIR"));
        assert_eq!(
            settings.log_max_files.to_string(),
            FieldSpec::default_value("LOG_MAX_FILES")
        );
    }

    #[test]
    fn catalog_defaults_are_accepted_by_settings() {
        let mut env = base();
        for field in FIELD_CATALOG {
            if let Some(default) = field.default {
                if field.key == "RUST_LOG" {
                    continue;
                }
                env.insert(field.key.to_string(), default.to_string());
            }
        }
        Settings::from_env_map(&env).unwrap();
    }

    #[test]
    fn env_example_mentions_every_catalog_key() {
        let example = include_str!("../../.env.example");
        for field in FIELD_CATALOG {
            assert!(
                example.contains(field.key),
                "{} missing from .env.example",
                field.key
            );
        }
    }

    #[test]
    fn every_field_has_operator_copy() {
        for field in FIELD_CATALOG {
            assert!(
                !field.summary.is_empty() && !field.explanation.is_empty(),
                "{} is missing operator copy",
                field.key
            );
            assert!(
                !field.constraints.is_empty(),
                "{} is missing constraints",
                field.key
            );
            if field.tier == FieldTier::Required {
                assert!(
                    field.how_to_get.is_some(),
                    "{} is required but has no how-to-get",
                    field.key
                );
            }
        }
    }
}
