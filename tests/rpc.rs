//! FR-FW-2 and NFR-1: the RPC client against fake JSON-RPC providers over real HTTP (wiremock).

// Test helpers outside #[test] functions may unwrap; a failure is a test failure.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use feedwatch_poller::abi::LATEST_ROUND_DATA;
use feedwatch_poller::manifest::Chain;
use feedwatch_poller::rpc::{Call, CallError, HttpTransport, RpcClient};
use serde_json::{Value, json};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Answers every call in a batch with `0x` plus its ID, in reverse order.
struct EchoBatch;

impl Respond for EchoBatch {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let calls: Vec<Value> = serde_json::from_slice(&request.body).unwrap();
        let mut answers: Vec<Value> = calls
            .iter()
            .map(|c| json!({"jsonrpc": "2.0", "id": c["id"], "result": format!("0x{:02x}", c["id"].as_u64().unwrap())}))
            .collect();
        answers.reverse();
        ResponseTemplate::new(200).set_body_json(answers)
    }
}

fn calls(n: usize) -> Vec<Call> {
    (0..n)
        .map(|i| Call {
            to: format!("0x{i:040x}"),
            data: LATEST_ROUND_DATA,
        })
        .collect()
}

fn client(providers: Vec<String>) -> RpcClient<HttpTransport> {
    let transport = HttpTransport::new(Duration::from_secs(2)).unwrap();
    RpcClient::with_backoff(
        transport,
        Chain::Base,
        providers,
        vec![Duration::ZERO, Duration::ZERO],
    )
}

#[tokio::test]
async fn fr_fw_2_fails_over_when_the_first_provider_errors() {
    let broken = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(3)
        .mount(&broken)
        .await;
    let healthy = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(EchoBatch)
        .expect(1)
        .mount(&healthy)
        .await;

    let outcome = client(vec![broken.uri(), healthy.uri()])
        .call_batch(&calls(3))
        .await;

    assert_eq!(
        outcome.results,
        vec![
            Ok("0x00".to_owned()),
            Ok("0x01".to_owned()),
            Ok("0x02".to_owned())
        ]
    );
    let oks: Vec<bool> = outcome.stats.iter().map(|s| s.ok).collect();
    assert_eq!(
        oks,
        [false, false, false, true],
        "two retries on the broken provider, then the healthy one"
    );
    assert!(
        outcome
            .stats
            .iter()
            .all(|s| s.provider == "127.0.0.1" && s.calls == 3 && s.chain == Chain::Base)
    );
}

#[tokio::test]
async fn fr_fw_2_reports_every_call_failed_when_no_provider_answers() {
    let broken = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&broken)
        .await;

    let outcome = client(vec![broken.uri()]).call_batch(&calls(2)).await;

    assert_eq!(outcome.results, vec![Err(CallError::AllProvidersFailed); 2]);
    assert_eq!(outcome.stats.len(), 3);
}

#[tokio::test]
async fn fr_fw_2_does_not_retry_a_rejected_request() {
    let unauthorised = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&unauthorised)
        .await;
    let healthy = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(EchoBatch)
        .mount(&healthy)
        .await;

    let outcome = client(vec![unauthorised.uri(), healthy.uri()])
        .call_batch(&calls(1))
        .await;

    assert_eq!(outcome.results, vec![Ok("0x00".to_owned())]);
    assert_eq!(outcome.stats.len(), 2);
}

#[tokio::test]
async fn fr_fw_2_survives_an_unreachable_provider() {
    let healthy = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(EchoBatch)
        .mount(&healthy)
        .await;

    let outcome = client(vec!["http://127.0.0.1:9".to_owned(), healthy.uri()])
        .call_batch(&calls(1))
        .await;

    assert_eq!(outcome.results, vec![Ok("0x00".to_owned())]);
}

#[tokio::test]
async fn nfr_1_sends_45_calls_as_three_batches_of_at_most_20() {
    let provider = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(EchoBatch)
        .expect(3)
        .mount(&provider)
        .await;

    let outcome = client(vec![provider.uri()]).call_batch(&calls(45)).await;

    assert_eq!(outcome.results.len(), 45);
    assert!(outcome.results.iter().all(Result::is_ok));
    let sizes: Vec<usize> = outcome.stats.iter().map(|s| s.calls).collect();
    assert_eq!(sizes, [20, 20, 5]);
    let requests = provider.received_requests().await.unwrap();
    let largest = requests
        .iter()
        .map(|r| serde_json::from_slice::<Vec<Value>>(&r.body).unwrap().len())
        .max();
    assert_eq!(largest, Some(20));
}
