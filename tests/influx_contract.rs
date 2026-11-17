//! FR-FW-5: every point the poller writes follows the InfluxDB data contract.
//!
//! The contract lives in feedwatch-platform `contracts/influx-schema.json`. A copy is vendored at
//! `tests/fixtures/influx-schema.json` so CI, which checks out only this repo, can test against it.
//! When the platform repo sits next to this one, the copy must match it.

// Test helpers outside #[test] functions may unwrap; a failure is a test failure.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::time::Duration;

use feedwatch_poller::abi::Round;
use feedwatch_poller::checks::{Assessment, IncidentEdge, IncidentEvent, Status};
use feedwatch_poller::influx::{
    CONTRACT_VERSION, Point, feed_check, feed_round, incident, poll_cycle, rpc_call,
};
use feedwatch_poller::manifest::{Address, Chain, Feed, Schedule};
use feedwatch_poller::rpc::RpcCallStat;
use serde_json::Value;

fn contract() -> Value {
    let path = format!(
        "{}/tests/fixtures/influx-schema.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn every_point() -> Vec<Point> {
    let triggers = Feed {
        chain: Chain::Base,
        id: "ETH".to_owned(),
        address: Address::parse("0x0000000000000000000000000000000000000001").unwrap(),
        schedule: Schedule::Triggers {
            heartbeat: Duration::from_secs(86_400),
            deviation_pct: 0.5,
        },
    };
    let cron = Feed {
        schedule: Schedule::Cron {
            expressions: vec!["0 10 * * *".to_owned()],
            period: Duration::from_secs(86_400),
        },
        ..triggers.clone()
    };
    let full = Assessment {
        status: Status::Drift,
        value: Some(1.0),
        age_s: Some(1),
        reference_deviation_pct: Some(1.0),
        detail: None,
    };
    let bare = Assessment {
        status: Status::Ok,
        reference_deviation_pct: None,
        ..full.clone()
    };
    let round = Round {
        round_id: 1,
        answer: 1,
        updated_at: 1,
    };
    let stat = RpcCallStat {
        chain: Chain::Base,
        provider: "h".to_owned(),
        latency_ms: 1,
        ok: true,
        calls: 1,
    };
    let event = IncidentEvent {
        chain: Chain::Base,
        feed_id: "ETH".to_owned(),
        kind: Status::Stale,
        edge: IncidentEdge::Open,
        incident_id: "base:ETH:1".to_owned(),
        detail: String::new(),
    };
    vec![
        feed_round(&triggers, &round, 1.0, 1, 1),
        feed_check(&triggers, &full, Some("chainlink"), 1),
        feed_check(&cron, &bare, None, 1),
        rpc_call(&stat, 1),
        incident(&event, 1),
        poll_cycle(1, 1, 0, 1),
    ]
}

#[test]
fn fr_fw_5_points_follow_the_contract() {
    let contract = contract();
    assert_eq!(contract["version"].as_u64(), Some(CONTRACT_VERSION));
    let mut seen = BTreeSet::new();
    for point in every_point() {
        let spec = &contract["measurements"][point.measurement];
        assert!(
            spec.is_object(),
            "{} is not in the contract",
            point.measurement
        );
        seen.insert(point.measurement);

        let tags: BTreeSet<&str> = point.tags.iter().map(|(k, _)| *k).collect();
        let expected: BTreeSet<&str> = spec["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_str().unwrap())
            .collect();
        assert_eq!(tags, expected, "{} tags", point.measurement);
        for (tag, value) in &point.tags {
            if let Some(allowed) = spec["tagValues"][*tag].as_array() {
                assert!(
                    allowed.iter().any(|a| a == value),
                    "{}.{tag}={value}",
                    point.measurement
                );
            }
        }

        let fields = spec["fields"].as_object().unwrap();
        for (name, _) in &point.fields {
            assert!(
                fields.contains_key(*name),
                "{}.{name} is not in the contract",
                point.measurement
            );
        }
        for (name, kind) in fields {
            let optional = kind.as_str().unwrap().ends_with('?');
            let written = point.fields.iter().any(|(f, _)| f == name);
            assert!(
                optional || written,
                "{}.{name} is required",
                point.measurement
            );
        }
    }
    let all: BTreeSet<&str> = contract["measurements"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(seen, all, "every measurement is written");
}

#[test]
fn vendored_contract_matches_the_platform_repo() {
    let platform = format!(
        "{}/../feedwatch-platform/contracts/influx-schema.json",
        env!("CARGO_MANIFEST_DIR")
    );
    match std::fs::read_to_string(platform) {
        Ok(text) => assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            contract(),
            "refresh tests/fixtures"
        ),
        Err(_) => eprintln!(
            "feedwatch-platform not checked out next to this repo; using the vendored contract"
        ),
    }
}
