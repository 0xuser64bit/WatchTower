//! ChainSentinel: a private Telegram-controlled Solana monitoring daemon.
//!
//! Two long-running halves share [`app_state::AppState`]:
//!
//! * the **control plane** ([`telegram`]) — long-polls Telegram, authorizes every
//!   update, and owns all mutations of the tracked directory and alert rules;
//! * the **data plane** ([`engine`]) — polls providers on a fixed interval, evaluates
//!   [`rules`], and hands firings to [`alerts`] for delivery.
//!
//! SQLite is the single source of truth for identity, targets, rules, and history.
//! Configuration only seeds it.

pub mod alerts;
pub mod app;
pub mod app_state;
pub mod config;
pub mod db;
pub mod engine;
pub mod error;
pub mod observability;
pub mod providers;
pub mod rules;
pub mod telegram;
