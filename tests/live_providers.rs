//! Live provider tests against the real CoinGecko and Solana APIs.
//!
//! Ignored by default: they need network access and depend on third-party
//! availability and rate limits, so they must never gate CI. Run them when changing
//! provider code or verifying a deployment's outbound connectivity:
//!
//! ```text
//! cargo test --test live_providers -- --ignored --nocapture
//! ```

use chainsentinel::config::Commitment;
use chainsentinel::providers::price::CoinGeckoProvider;
use chainsentinel::providers::solana::SolanaRpcProvider;
use chainsentinel::providers::{ChainProvider, PriceProvider};
use std::time::Duration;

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
        matches!(err, chainsentinel::providers::ProviderError::Unsupported(_)),
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
