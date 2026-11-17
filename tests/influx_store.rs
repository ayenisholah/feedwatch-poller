//! FR-FW-5 against a real InfluxDB 2 in Docker (testcontainers): points are accepted, and an
//! incident left open is found again after a restart.
//!
//! Needs Docker, so it's `#[ignore]` locally. `CI=1 ./scripts/check.sh` runs it, as CI does.

// Test helpers outside #[test] functions may unwrap; a failure is a test failure.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use feedwatch_poller::checks::{IncidentEdge, IncidentEvent, OpenIncident, Status};
use feedwatch_poller::influx::{
    HttpInflux, InfluxConfig, InfluxStore, incident, line_protocol, poll_cycle,
};
use feedwatch_poller::manifest::Chain;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

const TOKEN: &str = "feedwatch-test-token";

async fn influx() -> (ContainerAsync<GenericImage>, HttpInflux) {
    let container = GenericImage::new("influxdb", "2.9.1")
        .with_exposed_port(8086.tcp())
        .with_wait_for(WaitFor::Nothing)
        .with_env_var("DOCKER_INFLUXDB_INIT_MODE", "setup")
        .with_env_var("DOCKER_INFLUXDB_INIT_USERNAME", "feedwatch")
        .with_env_var("DOCKER_INFLUXDB_INIT_PASSWORD", "feedwatch-test")
        .with_env_var("DOCKER_INFLUXDB_INIT_ORG", "feedwatch")
        .with_env_var("DOCKER_INFLUXDB_INIT_BUCKET", "feedwatch")
        .with_env_var("DOCKER_INFLUXDB_INIT_ADMIN_TOKEN", TOKEN)
        .start()
        .await
        .unwrap();
    let port = container.get_host_port_ipv4(8086).await.unwrap();
    let config = InfluxConfig {
        url: format!("http://127.0.0.1:{port}"),
        org: "feedwatch".to_owned(),
        bucket: "feedwatch".to_owned(),
        token: TOKEN.to_owned(),
    };
    let store = HttpInflux::new(config, Duration::from_secs(5)).unwrap();
    // Setup mode restarts influxd once; wait until a write is accepted.
    for _ in 0..60 {
        if store
            .write(line_protocol(&[poll_cycle(1, 0, 0, now())]))
            .await
            .is_ok()
        {
            return (container, store);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("InfluxDB never accepted a write");
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn event(feed: &str, edge: IncidentEdge, id: &str) -> IncidentEvent {
    IncidentEvent {
        chain: Chain::Ethereum,
        feed_id: feed.to_owned(),
        kind: Status::Error,
        edge,
        incident_id: id.to_owned(),
        detail: "reverted: execution reverted".to_owned(),
    }
}

#[tokio::test]
#[ignore = "needs Docker; run with CI=1 ./scripts/check.sh"]
async fn fr_fw_5_open_incidents_survive_a_restart() {
    let (_container, store) = influx().await;
    let t = now() - 120;
    let lines = line_protocol(&[
        incident(&event("USUAL", IncidentEdge::Open, "ethereum:USUAL:1"), t),
        incident(&event("ETH", IncidentEdge::Open, "ethereum:ETH:2"), t),
        incident(&event("ETH", IncidentEdge::Close, "ethereum:ETH:2"), t + 60),
    ]);
    store.write(lines).await.unwrap();

    let open = store.open_incidents().await.unwrap();

    assert_eq!(
        open,
        vec![(
            Chain::Ethereum,
            "USUAL".to_owned(),
            OpenIncident {
                incident_id: "ethereum:USUAL:1".to_owned(),
                kind: Status::Error
            }
        )]
    );
}

#[tokio::test]
#[ignore = "needs Docker; run with CI=1 ./scripts/check.sh"]
async fn fr_fw_5_rejected_writes_are_reported() {
    let (_container, store) = influx().await;
    let error = store
        .write("not line protocol".to_owned())
        .await
        .unwrap_err();
    assert!(error.starts_with("write failed: HTTP 400"), "{error}");
}
