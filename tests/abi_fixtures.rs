//! FR-FW-2: decoding real `eth_call` results, recorded from the RedStone ETH feed on Ethereum
//! (`0x67F6838e58859d612E4ddF04dA396d6DABB66Dc4`) through publicnode on 2026-11-17.

// Test helpers outside #[test] functions may unwrap; a failure is a test failure.
#![allow(clippy::unwrap_used)]

use feedwatch_poller::abi::{decode_decimals, decode_round};
use serde_json::Value;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
    .trim()
    .to_owned()
}

#[test]
fn fr_fw_2_decodes_a_recorded_eth_round() {
    let expected: Value = serde_json::from_str(&fixture("eth-expected.json")).unwrap();
    let round = decode_round(&fixture("eth-latest-round-data.hex")).unwrap();
    let decimals = decode_decimals(&fixture("eth-decimals.hex")).unwrap();

    assert_eq!(round.answer_raw(), expected["answer"].as_str().unwrap());
    assert_eq!(
        round.round_id.to_string(),
        expected["roundId"].as_str().unwrap()
    );
    assert_eq!(round.updated_at, expected["updatedAt"].as_u64().unwrap());
    assert_eq!(u64::from(decimals), expected["decimals"].as_u64().unwrap());
    assert!((round.scaled(decimals) - 1_806.339_502_35).abs() < 1e-9);
}
