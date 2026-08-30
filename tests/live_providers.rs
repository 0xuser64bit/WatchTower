//! Live provider tests against the real CoinGecko and Solana APIs.
//!
//! Ignored by default: they need network access and depend on third-party
//! availability and rate limits, so they must never gate CI. Run them when changing
//! provider code or verifying a deployment's outbound connectivity:
//!
//! ```text
//! cargo test --test live_providers -- --ignored --nocapture
//! ```

use std::time::Duration;
use watchtower::config::Commitment;
use watchtower::providers::price::CoinGeckoProvider;
use watchtower::providers::solana::SolanaRpcProvider;
use watchtower::providers::{ChainProvider, PriceProvider};

/// USDC, which is reliably listed and priced near 1 USD.
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
/// The SPL Token program account: always exists and holds a non-zero balance.
const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
/// A valid, deterministically-generated address with no account on chain. Verified
/// unfunded when written; if it ever receives lamports, regenerate it.
const UNFUNDED: &str = "CZecYkamnAJKs6g2s4uoykkrstweT6XWu5zi9bdJiaS8";

fn timeout() -> Duration {
    Duration::from_secs(20)
}

#[tokio::test]
#[ignore = "requires network access to api.coingecko.com"]
async fn reads_a_real_token_price() {
    let provider = CoinGeckoProvider::new(
        &["https://api.coingecko.com/api/v3".to_string()],
        None,
        timeout(),
    )
    .unwrap();

    let price = provider.get_token_price_usd(USDC_MINT).await.unwrap();
    println!("USDC price: {price}");

    // A stablecoin, so a wide sanity band still proves the value is real.
    assert!(
        (0.5..2.0).contains(&price),
        "implausible USDC price: {price}"
    );
}

#[tokio::test]
#[ignore = "requires network access to api.coingecko.com"]
async fn an_unlisted_mint_is_reported_as_unsupported() {
    let provider = CoinGeckoProvider::new(
        &["https://api.coingecko.com/api/v3".to_string()],
        None,
        timeout(),
    )
    .unwrap();

    // A well-formed address CoinGecko cannot possibly have a listing for.
    let err = provider.get_token_price_usd(UNFUNDED).await.unwrap_err();

    println!("unlisted mint error: {err}");
    assert!(
        matches!(err, watchtower::providers::ProviderError::Unsupported(_)),
        "{err}"
    );
}

#[tokio::test]
#[ignore = "requires network access to a Solana RPC endpoint"]
async fn reads_a_real_balance() {
    let provider = SolanaRpcProvider::new(
        vec!["https://api.mainnet-beta.solana.com".to_string()],
        Commitment::Confirmed,
        timeout(),
    )
    .unwrap();

    let lamports = provider
        .get_native_balance_lamports(TOKEN_PROGRAM)
        .await
        .unwrap();

    println!("token program balance: {lamports} lamports");
    assert!(lamports > 0);
}

#[tokio::test]
#[ignore = "requires network access to a Solana RPC endpoint"]
async fn batches_real_balances_and_treats_missing_accounts_as_zero() {
    let provider = SolanaRpcProvider::new(
        vec!["https://api.mainnet-beta.solana.com".to_string()],
        Commitment::Confirmed,
        timeout(),
    )
    .unwrap();

    let addresses = vec![
        TOKEN_PROGRAM.to_string(),
        UNFUNDED.to_string(),
        USDC_MINT.to_string(),
    ];

    let balances = provider
        .get_native_balances_lamports(&addresses)
        .await
        .unwrap();

    println!("batched balances: {balances:?}");
    assert_eq!(balances.len(), addresses.len());
    assert!(balances[0] > 0, "token program should hold lamports");
    assert_eq!(balances[1], 0, "an unfunded account is a zero balance");
    assert!(
        balances[2] > 0,
        "the USDC mint account should hold lamports"
    );
}

#[tokio::test]
#[ignore = "requires network access to a Solana RPC endpoint"]
async fn every_catalog_mint_is_an_initialised_mint_on_mainnet() {
    // The catalog is offered as trustworthy, compiled-in data: a transposed character
    // would ship a button that can never work, and the failure would look like a
    // provider outage rather than a bad address. This is the check that a reviewer
    // cannot do by eye.
    use serde_json::{json, Value};

    let mints: Vec<&str> = watchtower::catalog::ENTRIES
        .iter()
        .map(|entry| entry.mint)
        .collect();

    let client = reqwest::Client::builder()
        .timeout(timeout())
        .build()
        .unwrap();
    let response: Value = client
        .post("https://api.mainnet-beta.solana.com")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getMultipleAccounts",
            "params": [mints, { "encoding": "jsonParsed" }],
        }))
        .send()
        .await
        .expect("rpc request")
        .json()
        .await
        .expect("rpc json");

    let accounts = response["result"]["value"]
        .as_array()
        .unwrap_or_else(|| panic!("unexpected rpc response: {response}"));
    assert_eq!(accounts.len(), mints.len());

    for (entry, account) in watchtower::catalog::ENTRIES.iter().zip(accounts) {
        assert!(
            !account.is_null(),
            "{} ({}) has no account on mainnet",
            entry.symbol,
            entry.mint
        );

        let parsed = &account["data"]["parsed"];
        assert_eq!(
            parsed["type"], "mint",
            "{} is not a mint account",
            entry.symbol
        );
        assert_eq!(
            parsed["info"]["isInitialized"], true,
            "{} is an uninitialised mint",
            entry.symbol
        );

        println!(
            "{:>9}  decimals={}  {}",
            entry.symbol, parsed["info"]["decimals"], entry.mint
        );
    }
}

/// One real monitoring cycle: real price API, real Solana RPC, real database, real
/// evaluation and persistence. Telegram delivery is expected to fail (no valid token),
/// which is itself worth exercising — a delivery failure must not lose the alert.
#[tokio::test]
#[ignore = "requires network access to CoinGecko and a Solana RPC endpoint"]
async fn a_real_monitoring_cycle_reads_evaluates_and_records() {
    use std::collections::HashMap;
    use std::sync::Arc;
    use watchtower::app_state::AppState;
    use watchtower::config::Settings;
    use watchtower::db::repos::alert_events::AlertEventRepo;
    use watchtower::db::repos::rules::{NewRuleTarget, RuleRepo};
    use watchtower::db::repos::tokens::TokenRepo;
    use watchtower::db::repos::users::{Role, UserRepo};
    use watchtower::db::repos::wallets::WalletRepo;
    use watchtower::db::Db;
    use watchtower::engine::scheduler;
    use watchtower::providers::price::CoinGeckoProvider;
    use watchtower::providers::solana::SolanaRpcProvider;
    use watchtower::rules::types::{Operator, RuleState};

    let db = Db::connect_in_memory().await.unwrap();
    db.migrate().await.unwrap();
    UserRepo::new(&db).upsert(1, Role::Admin).await.unwrap();
    let db = Arc::new(db);

    let settings = Settings::from_env_map(&HashMap::from([
        (
            "TELEGRAM_BOT_TOKEN".to_string(),
            "1234567890:live-test-token".to_string(),
        ),
        ("ADMIN_TELEGRAM_IDS".to_string(), "1".to_string()),
    ]))
    .unwrap();

    let price =
        Arc::new(CoinGeckoProvider::new(&settings.coingecko_api_urls, None, timeout()).unwrap());
    let chain = Arc::new(
        SolanaRpcProvider::new(
            settings.solana_rpc_endpoints.clone(),
            settings.solana_rpc_commitment,
            timeout(),
        )
        .unwrap(),
    );

    let state = AppState::new(
        db.clone(),
        teloxide::Bot::new(settings.telegram_bot_token.expose()),
        Arc::new(settings),
        price,
        chain,
        tokio_util::sync::CancellationToken::new(),
    );

    // USDC below $2 and the token program above 0 SOL: both true right now, so both
    // rules must fire on the first cycle.
    let token = TokenRepo::new(&state.db)
        .create(USDC_MINT, Some("USDC"))
        .await
        .unwrap();
    let wallet = WalletRepo::new(&state.db)
        .create(TOKEN_PROGRAM, Some("Token program"))
        .await
        .unwrap();

    let repo = RuleRepo::new(&state.db);
    repo.create(NewRuleTarget::Token { id: token.id }, Operator::Lt, 2.0, 0)
        .await
        .unwrap();
    repo.create(
        NewRuleTarget::Wallet { id: wallet.id },
        Operator::Gt,
        0.000_001,
        0,
    )
    .await
    .unwrap();

    let report = scheduler::tick(&state).await.unwrap();
    println!("live tick: {report:?}");

    assert_eq!(report.rules_evaluated, 2, "both targets should be readable");
    assert_eq!(report.targets_unavailable, 0);
    assert_eq!(report.alerts_sent, 2);

    for rule in repo.list_all().await.unwrap() {
        println!(
            "  rule {} {:?} last_value={:?} state={:?}",
            rule.id, rule.target.kind, rule.last_value, rule.state
        );
        assert!(rule.last_value.is_some(), "rule {} has no reading", rule.id);
        assert_eq!(rule.state, RuleState::Firing);
        assert!(rule.last_triggered_at.is_some());
    }

    // Delivery failed (the token is not real), but the alerts must still be recorded.
    assert_eq!(AlertEventRepo::new(&state.db).count().await.unwrap(), 2);

    // A second cycle must be silent: the conditions are unchanged.
    let report = scheduler::tick(&state).await.unwrap();
    assert_eq!(report.alerts_sent, 0);
    assert_eq!(AlertEventRepo::new(&state.db).count().await.unwrap(), 2);
}
