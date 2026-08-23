//! Live-check HTTP behaviour against a mock server. Secrets must never appear in
//! error strings.

use std::time::Duration;
use watchtower::setup::{HttpLiveChecker, LiveChecker};

const TOKEN: &str = "1234567890:AAEhBOweik6ad";
const TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn telegram_get_me_returns_bot_identity() {
    let mut server = mockito::Server::new_async().await;
    let path = format!("/bot{TOKEN}/getMe");
    let mock = server
        .mock("GET", path.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true,"result":{"id":99,"is_bot":true,"username":"watchtower_bot"}}"#)
        .create_async()
        .await;

    let checker = HttpLiveChecker::with_telegram_base(TIMEOUT, &server.url()).unwrap();
    let bot = checker.telegram_get_me(TOKEN).await.unwrap();
    mock.assert_async().await;
    assert_eq!(bot.id, 99);
    assert_eq!(bot.username, "watchtower_bot");
}

#[tokio::test]
async fn telegram_unauthorized_is_rejected_without_leaking_the_token() {
    let mut server = mockito::Server::new_async().await;
    let path = format!("/bot{TOKEN}/getMe");
    let _mock = server
        .mock("GET", path.as_str())
        .with_status(401)
        .with_body(r#"{"ok":false,"description":"Unauthorized"}"#)
        .create_async()
        .await;

    let checker = HttpLiveChecker::with_telegram_base(TIMEOUT, &server.url()).unwrap();
    let err = checker.telegram_get_me(TOKEN).await.unwrap_err();
    let rendered = err.to_string();
    assert!(rendered.contains("rejected"), "{rendered}");
    assert!(!rendered.contains(TOKEN), "{rendered}");
}

#[tokio::test]
async fn coingecko_ping_sends_demo_key_header() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/ping")
        .match_header("x-cg-demo-api-key", "demo-key")
        .with_status(200)
        .with_body(r#"{"gecko_says":"(V3) To the Moon!"}"#)
        .create_async()
        .await;

    let checker = HttpLiveChecker::with_telegram_base(TIMEOUT, &server.url()).unwrap();
    checker
        .ping_coingecko(&server.url(), Some("demo-key"))
        .await
        .unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn solana_get_slot_success() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","id":1,"result":123}"#)
        .create_async()
        .await;

    let checker = HttpLiveChecker::with_telegram_base(TIMEOUT, &server.url()).unwrap();
    checker.ping_solana_rpc(&server.url()).await.unwrap();
    mock.assert_async().await;
}

#[tokio::test]
async fn solana_rpc_error_is_rejected() {
    let mut server = mockito::Server::new_async().await;
    let _mock = server
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#,
        )
        .create_async()
        .await;

    let checker = HttpLiveChecker::with_telegram_base(TIMEOUT, &server.url()).unwrap();
    let err = checker.ping_solana_rpc(&server.url()).await.unwrap_err();
    assert!(err.to_string().contains("Method not found"), "{err}");
}
