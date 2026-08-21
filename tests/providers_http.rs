//! HTTP-level provider tests against a mock server.
//!
//! Covers the behaviour that decides whether alerting keeps working under real
//! conditions: rate limits, server errors, failover, and batching.

use chainsentinel::config::Commitment;
use chainsentinel::providers::price::CoinGeckoProvider;
use chainsentinel::providers::solana::SolanaRpcProvider;
use chainsentinel::providers::{ChainProvider, PriceProvider, ProviderError};
use mockito::Matcher;
use std::time::Duration;

const MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const TIMEOUT: Duration = Duration::from_secs(5);

fn price_provider(urls: &[String]) -> CoinGeckoProvider {
    CoinGeckoProvider::new(urls, None, TIMEOUT).unwrap()
}

fn token_price_query() -> Matcher {
    Matcher::AllOf(vec![
        Matcher::UrlEncoded("contract_addresses".into(), MINT.into()),
        Matcher::UrlEncoded("vs_currencies".into(), "usd".into()),
    ])
}

#[tokio::test]
async fn reads_a_token_price() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("GET", "/simple/token_price/solana")
        .match_query(token_price_query())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"{MINT}":{{"usd":0.999893}}}}"#))
        .create_async()
        .await;

    let price = price_provider(&[server.url()])
        .get_token_price_usd(MINT)
        .await
        .unwrap();

    mock.assert_async().await;
    assert_eq!(price, 0.999893);
}

#[tokio::test]
async fn an_unlisted_mint_is_reported_as_unsupported_not_as_a_parse_error() {
    let mut server = mockito::Server::new_async().await;

    // CoinGecko answers 200 with an empty object for a mint it does not know.
    let _mock = server
        .mock("GET", "/simple/token_price/solana")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("{}")
        .expect(1)
        .create_async()
        .await;

    let err = price_provider(&[server.url()])
        .get_token_price_usd(MINT)
        .await
        .unwrap_err();

    assert!(matches!(err, ProviderError::Unsupported(_)), "{err}");
    // Must not be retried: the answer will never change.
    assert!(!err.is_retryable());
}

#[tokio::test]
async fn a_rate_limit_is_recognised_rather_than_parsed_as_json() {
    let mut server = mockito::Server::new_async().await;

    // The original code ignored HTTP status entirely and fed the 429 body to the JSON
    // parser, so genuine rate limiting surfaced as an unhelpful deserialisation error.
    let mock = server
        .mock("GET", "/simple/token_price/solana")
        .match_query(Matcher::Any)
        .with_status(429)
        .with_header("retry-after", "1")
        .with_body(r#"{"status":{"error_code":429}}"#)
        .expect(3)
        .create_async()
        .await;

    let err = price_provider(&[server.url()])
        .get_token_price_usd(MINT)
        .await
        .unwrap_err();

    // Retried up to the attempt budget, then surfaced as a rate limit.
    mock.assert_async().await;
    assert!(
        matches!(
            err,
            ProviderError::RateLimited {
                retry_after: Some(_)
            }
        ),
        "{err}"
    );
}

#[tokio::test]
async fn a_client_error_is_not_retried() {
    let mut server = mockito::Server::new_async().await;

    // 400 is what CoinGecko returns for an over-limit request; retrying is pointless.
    let mock = server
        .mock("GET", "/simple/token_price/solana")
        .match_query(Matcher::Any)
        .with_status(400)
        .with_body("bad request")
        .expect(1)
        .create_async()
        .await;

    let err = price_provider(&[server.url()])
        .get_token_price_usd(MINT)
        .await
        .unwrap_err();

    mock.assert_async().await;
    assert!(matches!(err, ProviderError::InvalidResponse(_)), "{err}");
}

#[tokio::test]
async fn a_transient_server_error_is_retried_before_giving_up() {
    let mut server = mockito::Server::new_async().await;

    // A 5xx is worth retrying, unlike a 4xx: the attempt budget must actually be used
    // rather than the first failure ending the poll for this mint.
    let mock = server
        .mock("GET", "/simple/token_price/solana")
        .match_query(Matcher::Any)
        .with_status(503)
        .expect(3)
        .create_async()
        .await;

    let err = price_provider(&[server.url()])
        .get_token_price_usd(MINT)
        .await
        .unwrap_err();

    mock.assert_async().await;
    assert!(matches!(err, ProviderError::Unavailable(_)), "{err}");
    assert!(err.is_retryable());
}

#[tokio::test]
async fn falls_over_to_the_next_configured_api_url() {
    let mut primary = mockito::Server::new_async().await;
    let mut secondary = mockito::Server::new_async().await;

    let _down = primary
        .mock("GET", "/simple/token_price/solana")
        .match_query(Matcher::Any)
        .with_status(500)
        .expect_at_least(1)
        .create_async()
        .await;

    let up = secondary
        .mock("GET", "/simple/token_price/solana")
        .match_query(token_price_query())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"{MINT}":{{"usd":7.25}}}}"#))
        .expect(1)
        .create_async()
        .await;

    let price = price_provider(&[primary.url(), secondary.url()])
        .get_token_price_usd(MINT)
        .await
        .unwrap();

    up.assert_async().await;
    assert_eq!(price, 7.25);
}

#[tokio::test]
async fn a_non_positive_price_is_rejected() {
    let mut server = mockito::Server::new_async().await;

    let _mock = server
        .mock("GET", "/simple/token_price/solana")
        .match_query(Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"{MINT}":{{"usd":0}}}}"#))
        .create_async()
        .await;

    assert!(matches!(
        price_provider(&[server.url()])
            .get_token_price_usd(MINT)
            .await
            .unwrap_err(),
        ProviderError::InvalidResponse(_)
    ));
}

fn rpc(urls: Vec<String>) -> SolanaRpcProvider {
    SolanaRpcProvider::new(urls, Commitment::Confirmed, TIMEOUT).unwrap()
}

#[tokio::test]
async fn reads_a_single_balance() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/")
        .match_body(Matcher::PartialJsonString(
            r#"{"method":"getBalance"}"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":2500000000}}"#)
        .create_async()
        .await;

    let lamports = rpc(vec![server.url()])
        .get_native_balance_lamports(MINT)
        .await
        .unwrap();

    mock.assert_async().await;
    assert_eq!(lamports, 2_500_000_000);
}

#[tokio::test]
async fn rejects_a_malformed_address_without_a_round_trip() {
    let mut server = mockito::Server::new_async().await;

    let never = server
        .mock("POST", "/")
        .expect(0)
        .with_body("{}")
        .create_async()
        .await;

    assert!(rpc(vec![server.url()])
        .get_native_balance_lamports("not-an-address")
        .await
        .is_err());

    never.assert_async().await;
}

#[tokio::test]
async fn batches_many_balances_into_one_request() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/")
        .match_body(Matcher::PartialJsonString(
            r#"{"method":"getMultipleAccounts"}"#.to_string(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":[
                {"lamports":1000000000,"owner":"11111111111111111111111111111111",
                 "data":["","base64"],"executable":false,"rentEpoch":0,"space":0},
                null,
                {"lamports":42,"owner":"11111111111111111111111111111111",
                 "data":["","base64"],"executable":false,"rentEpoch":0,"space":0}]}}"#,
        )
        .expect(1)
        .create_async()
        .await;

    let addresses = vec![MINT.to_string(), "a".to_string(), "b".to_string()];
    let balances = rpc(vec![server.url()])
        .get_native_balances_lamports(&addresses)
        .await
        .unwrap();

    mock.assert_async().await;
    // A null account is a real zero balance on Solana, not an unknown value.
    assert_eq!(balances, vec![1_000_000_000, 0, 42]);
}

#[tokio::test]
async fn a_truncated_batch_response_is_an_error_not_a_silent_misalignment() {
    let mut server = mockito::Server::new_async().await;

    // Returning fewer entries than requested would otherwise zip wallet ids to the
    // wrong balances and alert on the wrong wallet.
    let _mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":[null]}}"#)
        .create_async()
        .await;

    let addresses = vec!["a".to_string(), "b".to_string()];
    assert!(matches!(
        rpc(vec![server.url()])
            .get_native_balances_lamports(&addresses)
            .await
            .unwrap_err(),
        ProviderError::InvalidResponse(_)
    ));
}

#[tokio::test]
async fn an_rpc_error_object_is_surfaced() {
    let mut server = mockito::Server::new_async().await;

    let _mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"Invalid param"}}"#)
        .create_async()
        .await;

    // -32602 is bad input rather than an unhealthy node, so it must not be retried
    // into oblivion or take the endpoint out of rotation.
    let err = rpc(vec![server.url()])
        .get_native_balances_lamports(&[MINT.to_string()])
        .await
        .unwrap_err();

    assert!(matches!(err, ProviderError::InvalidResponse(_)), "{err}");
    assert!(!err.is_retryable());
}

#[tokio::test]
async fn an_unhealthy_endpoint_is_skipped_in_favour_of_a_healthy_one() {
    let mut bad = mockito::Server::new_async().await;
    let mut good = mockito::Server::new_async().await;

    let _down = bad
        .mock("POST", "/")
        .with_status(500)
        .expect_at_least(1)
        .create_async()
        .await;

    let up = good
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":7}}"#)
        .expect_at_least(1)
        .create_async()
        .await;

    let provider = rpc(vec![bad.url(), good.url()]);

    // First call may hit the bad endpoint and retry onto the good one.
    assert_eq!(provider.get_native_balance_lamports(MINT).await.unwrap(), 7);

    // The bad endpoint is now benched, so subsequent calls go straight to the good one.
    for _ in 0..3 {
        assert_eq!(provider.get_native_balance_lamports(MINT).await.unwrap(), 7);
    }

    up.assert_async().await;
}
