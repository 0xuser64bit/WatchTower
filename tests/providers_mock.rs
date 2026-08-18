use chainsentinel::providers::price::coingecko::CoinGeckoProvider;
use chainsentinel::providers::PriceProvider;
use mockito::Server;

#[tokio::test]
async fn fetches_native_price() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let mock = server
        .mock("GET", "/simple/price")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("ids".into(), "solana".into()),
            mockito::Matcher::UrlEncoded("vs_currencies".into(), "usd".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"solana":{"usd":145.67}}"#)
        .create_async()
        .await;

    let provider = CoinGeckoProvider::new(&url, &[]).unwrap();
    let price = provider.get_native_price_usd().await.unwrap();

    mock.assert_async().await;
    assert_eq!(price, 145.67);
}

#[tokio::test]
async fn falls_back_when_primary_fails() {
    let mut primary = Server::new_async().await;
    let mut fallback = Server::new_async().await;

    let primary_url = primary.url();
    let fallback_url = fallback.url();

    let _primary_mock = primary
        .mock("GET", "/simple/price")
        .with_status(500)
        .create_async()
        .await;

    let fallback_mock = fallback
        .mock("GET", "/simple/price")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("ids".into(), "solana".into()),
            mockito::Matcher::UrlEncoded("vs_currencies".into(), "usd".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"solana":{"usd":99.5}}"#)
        .create_async()
        .await;

    let provider = CoinGeckoProvider::new(&primary_url, &[fallback_url.clone()]).unwrap();
    let price = provider.get_native_price_usd().await.unwrap();

    fallback_mock.assert_async().await;
    assert_eq!(price, 99.5);
}
