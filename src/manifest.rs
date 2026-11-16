//! RedStone relayer manifests, vendored in `manifests/` (feedwatch-platform ADR 0004).
//!
//! Delivers FR-FW-1. Done so far: parsing. Next (`feat/manifest`): merge each feed's
//! `updateTriggersOverrides` into the manifest defaults and return one `Feed` per entry.

use std::collections::BTreeMap;

use serde::Deserialize;

/// A chain Feedwatch watches, with the manifest file vendored for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chain {
    /// Ethereum mainnet (chain ID 1).
    Ethereum,
    /// Arbitrum One (chain ID 42161).
    Arbitrum,
    /// Base (chain ID 8453).
    Base,
}

impl Chain {
    /// Every chain, in display order.
    pub const ALL: [Chain; 3] = [Chain::Ethereum, Chain::Arbitrum, Chain::Base];

    /// The tag value used in InfluxDB and the API (feedwatch-platform `docs/DATA_CONTRACT.md`).
    pub fn as_str(self) -> &'static str {
        match self {
            Chain::Ethereum => "ethereum",
            Chain::Arbitrum => "arbitrum",
            Chain::Base => "base",
        }
    }

    /// The vendored manifest's JSON text, compiled into the binary.
    pub fn vendored_manifest(self) -> &'static str {
        match self {
            Chain::Ethereum => include_str!("../manifests/ethereumMultiFeed.json"),
            Chain::Arbitrum => include_str!("../manifests/arbitrumOneMultiFeed.json"),
            Chain::Base => include_str!("../manifests/baseMultiFeed.json"),
        }
    }
}

/// A relayer manifest as RedStone publishes it. Unknown fields are ignored on purpose:
/// RedStone adds relayer-only settings that Feedwatch doesn't use.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawManifest {
    /// Chain name and ID.
    pub chain: RawChain,
    /// Default update triggers for every feed in the file.
    pub update_triggers: RawTriggers,
    /// The multi-feed adapter contract.
    pub adapter_contract: String,
    /// Feeds by RedStone feed ID.
    pub price_feeds: BTreeMap<String, RawFeed>,
}

/// The manifest's chain block.
#[derive(Debug, Clone, Deserialize)]
pub struct RawChain {
    /// RedStone's chain name, such as `mainnet` or `arbitrumOne`.
    pub name: String,
    /// EVM chain ID.
    pub id: u64,
}

/// Update triggers: a deviation threshold and a heartbeat. Overrides may set either one.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTriggers {
    /// Price move, in percent, that should trigger an update.
    pub deviation_percentage: Option<f64>,
    /// Heartbeat in milliseconds.
    pub time_since_last_update_in_milliseconds: Option<u64>,
}

/// One feed entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawFeed {
    /// The Chainlink-compatible price feed contract.
    pub price_feed_address: String,
    /// Per-feed trigger overrides, if any.
    #[serde(default)]
    pub update_triggers_overrides: Option<RawTriggers>,
}

/// Why a manifest couldn't be read.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The JSON didn't match the manifest shape.
    #[error("manifest is not valid: {0}")]
    Invalid(#[from] serde_json::Error),
}

/// Parses a manifest's JSON text.
pub fn parse_manifest(json: &str) -> Result<RawManifest, ManifestError> {
    Ok(serde_json::from_str(json)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fr_fw_1_manifests_parse() {
        let expected = [
            (Chain::Ethereum, 1, 95),
            (Chain::Arbitrum, 42161, 21),
            (Chain::Base, 8453, 30),
        ];
        for (chain, chain_id, feeds) in expected {
            let manifest = parse_manifest(chain.vendored_manifest()).unwrap();
            assert_eq!(manifest.chain.id, chain_id, "{}", chain.as_str());
            assert_eq!(manifest.price_feeds.len(), feeds, "{}", chain.as_str());
            assert_eq!(manifest.update_triggers.deviation_percentage, Some(0.5));
            assert_eq!(
                manifest
                    .update_triggers
                    .time_since_last_update_in_milliseconds,
                Some(86_400_000)
            );
        }
    }

    #[test]
    fn fr_fw_1_rejects_a_manifest_without_feeds() {
        let error = parse_manifest(r#"{"chain":{"name":"x","id":1}}"#).unwrap_err();
        assert!(error.to_string().starts_with("manifest is not valid"));
    }

    #[test]
    fn chain_tags_match_the_data_contract() {
        let tags: Vec<_> = Chain::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(tags, ["ethereum", "arbitrum", "base"]);
    }
}
