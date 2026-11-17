//! FR-FW-4: reference prices from a fake Chainlink node and a fake Coinbase API (wiremock).

// Test helpers outside #[test] functions may unwrap; a failure is a test failure.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use feedwatch_poller::manifest::Chain;
use feedwatch_poller::reference::{Asset, CoinbaseSpot, ReferenceBook};
use feedwatch_poller::rpc::{HttpTransport, RpcClient};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const NOW: u64 = 1_800_000_000;

/// A Chainlink node answering ETH 1800, BTC 64000 and USDC 1 (8 decimals), each last updated
/// `updated_ago` seconds before NOW.
struct Chainlink {
    updated_ago: [u64; 3],
}

impl Respond for Chainlink {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let calls: Vec<Value> = serde_json::from_slice(&request.body).unwrap();
        let answers: Vec<Value> = calls
            .iter()
            .map(|c| {
                let id = c["id"].as_u64().unwrap();
                let asset = (id / 2) as usize;
                let result = if c["params"][0]["data"] == "0x313ce567" {
                    format!("0x{:064x}", 8)
                } else {
                    let price = [180_000_000_000u128, 6_400_000_000_000, 100_000_000][asset];
                    let updated = NOW - self.updated_ago[asset];
                    format!(
                        "0x{:064x}{price:064x}{:064x}{updated:064x}{:064x}",
                        1, updated, 1
                    )
                };
                json!({"jsonrpc": "2.0", "id": id, "result": result})
            })
            .collect();
        ResponseTemplate::new(200).set_body_json(answers)
    }
}

async fn coinbase(prices: &[(&str, &str)]) -> MockServer {
    let server = MockServer::start().await;
    for (pair, amount) in prices {
        Mock::given(method("GET"))
            .and(path(format!("/v2/prices/{pair}/spot")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"data": {"amount": amount}})),
            )
            .mount(&server)
            .await;
    }
    server
}

async fn book(
    chainlink: &MockServer,
    coinbase: &MockServer,
) -> ReferenceBook<HttpTransport, CoinbaseSpot> {
    let transport = HttpTransport::new(Duration::from_secs(2)).unwrap();
    let rpc = RpcClient::with_backoff(transport, Chain::Ethereum, vec![chainlink.uri()], vec![]);
    ReferenceBook::new(
        rpc,
        CoinbaseSpot::new(coinbase.uri(), Duration::from_secs(2)).unwrap(),
    )
}

#[tokio::test]
async fn fr_fw_4_prefers_fresh_chainlink_prices() {
    let node = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(Chainlink {
            updated_ago: [60, 60, 60],
        })
        .expect(1)
        .mount(&node)
        .await;
    let spot = coinbase(&[]).await;
    let mut book = book(&node, &spot).await;

    let (prices, stats) = book.prices(NOW).await;

    assert_eq!(prices[&Asset::Eth].price, 1_800.0);
    assert_eq!(prices[&Asset::Btc].price, 64_000.0);
    assert!(prices.values().all(|r| r.source == "chainlink"));
    assert_eq!(
        stats.len(),
        1,
        "one batch for three feeds and their decimals"
    );
}

#[tokio::test]
async fn fr_fw_4_falls_back_to_coinbase_when_chainlink_is_too_old() {
    let node = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(Chainlink {
            updated_ago: [2 * 3_600, 60, 12 * 3_600],
        })
        .mount(&node)
        .await;
    let spot = coinbase(&[("ETH-USD", "1803.79")]).await;
    let mut book = book(&node, &spot).await;

    let (prices, _) = book.prices(NOW).await;

    assert_eq!(
        (prices[&Asset::Eth].price, prices[&Asset::Eth].source),
        (1803.79, "coinbase")
    );
    assert_eq!(prices[&Asset::Btc].source, "chainlink");
    assert_eq!(
        prices[&Asset::Usdc].source,
        "chainlink",
        "12 h is within USDC's day"
    );
}

#[tokio::test]
async fn fr_fw_4_has_no_reference_when_both_sources_fail() {
    let node = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&node)
        .await;
    let spot = coinbase(&[("BTC-USD", "64345.55")]).await;
    let mut book = book(&node, &spot).await;

    let (prices, stats) = book.prices(NOW).await;

    assert_eq!(prices.len(), 1);
    assert_eq!(prices[&Asset::Btc].source, "coinbase");
    assert!(!stats[0].ok);
}

#[tokio::test]
async fn fr_fw_4_reuses_prices_for_30_seconds() {
    let node = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(Chainlink {
            updated_ago: [60, 60, 60],
        })
        .expect(2)
        .mount(&node)
        .await;
    let spot = coinbase(&[]).await;
    let mut book = book(&node, &spot).await;

    book.prices(NOW).await;
    let (cached, stats) = book.prices(NOW + 29).await;
    assert_eq!(cached.len(), 3);
    assert!(stats.is_empty(), "served from the cache");
    let (_, refreshed) = book.prices(NOW + 30).await;
    assert_eq!(refreshed.len(), 1);
}
