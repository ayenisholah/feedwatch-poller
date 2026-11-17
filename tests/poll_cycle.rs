//! NFR-1 and FR-FW-2 to FR-FW-5 together: full poll cycles over the real 146-feed manifests, with
//! an in-memory RPC node, spot source and InfluxDB, plus one timing run over real HTTP.

// Test helpers outside #[test] functions may unwrap; a failure is a test failure.
#![allow(clippy::unwrap_used)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use feedwatch_poller::checks::{IncidentTracker, OpenIncident, Status};
use feedwatch_poller::influx::InfluxStore;
use feedwatch_poller::manifest::{Chain, load_feeds};
use feedwatch_poller::poller::{ChainReader, Poller};
use feedwatch_poller::reference::{Asset, ReferenceBook, SpotSource};
use feedwatch_poller::rpc::{
    HttpTransport, RpcClient, RpcTransport, TransportError, TransportResponse,
};
use serde_json::{Value, json};

const NOW: u64 = 1_800_000_000;

/// Answers every `latestRoundData` with 100.0 updated a minute ago and every `decimals` with 8.
/// URLs starting `down:` fail; addresses in `reverting` revert. Counts calls by selector.
#[derive(Clone, Default)]
struct FakeNode {
    reverting: Arc<HashSet<String>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl RpcTransport for FakeNode {
    async fn post(&self, url: &str, body: String) -> Result<TransportResponse, TransportError> {
        if url.starts_with("down:") {
            return Err(TransportError("connection refused".to_owned()));
        }
        let calls: Vec<Value> = serde_json::from_str(&body).unwrap();
        let answers: Vec<Value> = calls
            .iter()
            .map(|c| {
                let to = c["params"][0]["to"].as_str().unwrap();
                let data = c["params"][0]["data"].as_str().unwrap();
                self.calls.lock().unwrap().push(data.to_owned());
                if self.reverting.contains(to) && data == "0xfeaf968c" {
                    return json!({"id": c["id"], "error": {"code": 3, "message": "execution reverted"}});
                }
                let result = if data == "0x313ce567" {
                    format!("0x{:064x}", 8)
                } else {
                    let updated = NOW - 60;
                    format!("0x{:064x}{:064x}{updated:064x}{updated:064x}{:064x}", 1, 10_000_000_000u128, 1)
                };
                json!({"id": c["id"], "result": result})
            })
            .collect();
        Ok(TransportResponse {
            status: 200,
            body: serde_json::to_vec(&answers).unwrap(),
        })
    }
}

/// Spot prices: 100.0 for every asset.
struct FakeSpot;

impl SpotSource for FakeSpot {
    async fn spot(&self, _asset: Asset) -> Result<f64, String> {
        Ok(100.0)
    }
}

/// Keeps every write; fails them all when `failing` is set.
#[derive(Clone, Default)]
struct FakeStore {
    writes: Arc<Mutex<Vec<String>>>,
    failing: bool,
}

impl InfluxStore for FakeStore {
    async fn write(&self, lines: String) -> Result<(), String> {
        if self.failing {
            return Err("HTTP 503".to_owned());
        }
        self.writes.lock().unwrap().push(lines);
        Ok(())
    }

    async fn open_incidents(&self) -> Result<Vec<(Chain, String, OpenIncident)>, String> {
        Ok(Vec::new())
    }
}

fn poller<T: RpcTransport + Clone>(
    transport: T,
    urls: [&str; 3],
    store: FakeStore,
) -> Poller<T, FakeSpot, FakeStore> {
    let chains = Chain::ALL
        .iter()
        .zip(urls)
        .map(|(chain, url)| {
            let client =
                RpcClient::with_backoff(transport.clone(), *chain, vec![url.to_owned()], vec![]);
            ChainReader::new(client, load_feeds(*chain).unwrap())
        })
        .collect();
    let chainlink =
        RpcClient::with_backoff(transport, Chain::Ethereum, vec![urls[0].to_owned()], vec![]);
    Poller::new(
        chains,
        ReferenceBook::new(chainlink, FakeSpot),
        store,
        IncidentTracker::default(),
    )
}

fn lines(store: &FakeStore, measurement: &str) -> Vec<String> {
    let writes = store.writes.lock().unwrap();
    writes
        .last()
        .unwrap()
        .lines()
        .filter(|l| {
            l.starts_with(&format!("{measurement},")) || l.starts_with(&format!("{measurement} "))
        })
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn fr_fw_2_a_cycle_writes_every_feed() {
    let store = FakeStore::default();
    let node = FakeNode::default();
    let mut poller = poller(
        node.clone(),
        ["http://eth", "http://arb", "http://base"],
        store.clone(),
    );

    let report = poller.cycle(NOW).await;

    assert_eq!((report.feeds, report.failed), (146, false));
    assert_eq!(lines(&store, "feed_round").len(), 146);
    assert_eq!(lines(&store, "feed_check").len(), 146);
    assert_eq!(lines(&store, "poll_cycle").len(), 1);
    assert!(lines(&store, "rpc_call").len() >= 3);
    assert_eq!(
        report.failed_feeds, 0,
        "100.0 against a 100.0 reference, a minute old"
    );
}

#[tokio::test]
async fn decimals_are_read_once() {
    let node = FakeNode::default();
    let mut poller = poller(
        node.clone(),
        ["http://eth", "http://arb", "http://base"],
        FakeStore::default(),
    );

    poller.cycle(NOW).await;
    let first = node
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|d| *d == "0x313ce567")
        .count();
    poller.cycle(NOW + 60).await;
    let total = node
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|d| *d == "0x313ce567")
        .count();

    assert_eq!(
        first,
        146 + 3,
        "every feed plus the three Chainlink references"
    );
    assert_eq!(
        total - first,
        3,
        "only the uncached Chainlink decimals are read again"
    );
}

#[tokio::test]
async fn fr_fw_5_a_reverting_feed_opens_one_incident() {
    let usual = load_feeds(Chain::Ethereum)
        .unwrap()
        .into_iter()
        .find(|f| f.id == "USUAL")
        .unwrap();
    let node = FakeNode {
        reverting: Arc::new(HashSet::from([usual.address.as_str().to_owned()])),
        ..FakeNode::default()
    };
    let store = FakeStore::default();
    let mut poller = poller(
        node,
        ["http://eth", "http://arb", "http://base"],
        store.clone(),
    );

    let report = poller.cycle(NOW).await;
    let incidents = lines(&store, "incident");
    poller.cycle(NOW + 60).await;

    assert_eq!(
        (report.failed_feeds, report.open_incidents, report.failed),
        (1, 1, false)
    );
    assert_eq!(incidents.len(), 1);
    assert!(incidents[0].contains("feed_id=USUAL,kind=error,event=open"));
    assert!(incidents[0].contains("reverted: execution reverted"));
    assert!(
        lines(&store, "incident").is_empty(),
        "still reverting: no new event"
    );
}

#[tokio::test]
async fn fr_fw_2_one_chain_down_fails_the_cycle_but_keeps_the_others() {
    let store = FakeStore::default();
    let mut poller = poller(
        FakeNode::default(),
        ["http://eth", "http://arb", "down:base"],
        store.clone(),
    );

    let report = poller.cycle(NOW).await;

    assert!(report.failed);
    assert_eq!(report.failed_feeds, 30);
    assert_eq!(lines(&store, "feed_round").len(), 95 + 21);
    let base_errors = lines(&store, "feed_check")
        .iter()
        .filter(|l| l.contains("chain=base") && l.contains("status=error"))
        .count();
    assert_eq!(base_errors, 30);
    assert_eq!(report.open_incidents, 30);
    assert!(Status::Error.as_str() == "error");
}

#[tokio::test]
async fn a_failed_write_fails_the_cycle() {
    let store = FakeStore {
        failing: true,
        ..FakeStore::default()
    };
    let mut poller = poller(
        FakeNode::default(),
        ["http://eth", "http://arb", "http://base"],
        store,
    );
    assert!(poller.cycle(NOW).await.failed);
}

#[tokio::test]
async fn nfr_1_a_full_cycle_over_http_with_50ms_latency_is_under_10s() {
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    struct Slow(FakeNode);
    impl Respond for Slow {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let body = String::from_utf8(request.body.clone()).unwrap();
            let response = futures::executor::block_on(self.0.post("http://fake", body)).unwrap();
            ResponseTemplate::new(200)
                .set_body_raw(response.body, "application/json")
                .set_delay(Duration::from_millis(50))
        }
    }
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("POST"))
        .respond_with(Slow(FakeNode::default()))
        .mount(&server)
        .await;
    let uri = server.uri();
    let mut poller = poller(
        HttpTransport::new(Duration::from_secs(5)).unwrap(),
        [uri.as_str(), uri.as_str(), uri.as_str()],
        FakeStore::default(),
    );

    let started = std::time::Instant::now();
    let report = poller.cycle(NOW).await;

    assert_eq!((report.feeds, report.failed), (146, false));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "took {:?}",
        started.elapsed()
    );
    let largest = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice::<Vec<Value>>(&r.body).unwrap().len())
        .max();
    assert_eq!(largest, Some(20));
}
