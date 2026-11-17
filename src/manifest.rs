//! RedStone relayer manifests, vendored in `manifests/` (feedwatch-platform ADR 0004).
//!
//! Delivers FR-FW-1: every feed on the three chains, with its update schedule. A feed's schedule is
//! the manifest's default triggers with the feed's `updateTriggersOverrides` merged field by field,
//! or a daily `cron` schedule when the override sets one.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use serde::Deserialize;

/// A chain Feedwatch watches, with the manifest file vendored for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

    /// The EVM chain ID the manifest must declare.
    pub fn chain_id(self) -> u64 {
        match self {
            Chain::Ethereum => 1,
            Chain::Arbitrum => 42_161,
            Chain::Base => 8_453,
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

impl fmt::Display for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A contract address: `0x` and 40 hex digits, kept as written in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Address(String);

impl Address {
    /// Parses an address, rejecting anything that isn't `0x` plus 40 hex digits.
    pub fn parse(text: &str) -> Option<Address> {
        let hex = text.strip_prefix("0x")?;
        (hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| Address(text.to_owned()))
    }

    /// The address as written, for JSON-RPC calls.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// When a feed is expected to update.
#[derive(Debug, Clone, PartialEq)]
pub enum Schedule {
    /// Pushed when the price moves by `deviation_pct`, and at least every `heartbeat`.
    Triggers {
        /// Longest expected gap between updates.
        heartbeat: Duration,
        /// Price move, in percent, that should trigger an update.
        deviation_pct: f64,
    },
    /// Pushed on a daily cron schedule. There's no deviation trigger, so only freshness is checked
    /// (feedwatch-platform `docs/VERIFIED_FACTS.md`, F-14).
    Cron {
        /// The cron expressions as written in the manifest.
        expressions: Vec<String>,
        /// Longest expected gap between updates: one day.
        period: Duration,
    },
}

impl Schedule {
    /// The longest expected gap between updates, which the freshness check uses.
    pub fn max_gap(&self) -> Duration {
        match self {
            Schedule::Triggers { heartbeat, .. } => *heartbeat,
            Schedule::Cron { period, .. } => *period,
        }
    }
}

/// One feed Feedwatch reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Feed {
    /// The chain the feed is on.
    pub chain: Chain,
    /// RedStone's feed ID, exactly as the manifest key, such as `ETH` or `pzETH_FUNDAMENTAL`.
    pub id: String,
    /// The Chainlink-compatible price feed contract.
    pub address: Address,
    /// When the feed should update.
    pub schedule: Schedule,
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

/// Update triggers. Defaults set the first two; an override may set any of them.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTriggers {
    /// Price move, in percent, that should trigger an update.
    pub deviation_percentage: Option<f64>,
    /// Heartbeat in milliseconds.
    pub time_since_last_update_in_milliseconds: Option<u64>,
    /// Cron expressions, which replace the other triggers.
    pub cron: Option<Vec<String>>,
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

/// Why a manifest couldn't be turned into feeds.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ManifestError {
    /// The JSON didn't match the manifest shape.
    #[error("manifest is not valid: {0}")]
    Invalid(String),
    /// The manifest is for a different chain than the one asked for.
    #[error("manifest for {chain} declares chain ID {found}")]
    WrongChain {
        /// The chain asked for.
        chain: Chain,
        /// The chain ID the manifest declares.
        found: u64,
    },
    /// The defaults are missing a heartbeat or deviation threshold.
    #[error("manifest for {0} has incomplete default triggers")]
    IncompleteDefaults(Chain),
    /// A feed's address isn't `0x` plus 40 hex digits.
    #[error("{chain} feed {feed} has an invalid address")]
    InvalidAddress {
        /// The chain.
        chain: Chain,
        /// The feed ID.
        feed: String,
    },
    /// A feed's cron schedule isn't a daily `M H * * *` expression.
    #[error("{chain} feed {feed} has an unsupported cron schedule {expression:?}")]
    UnsupportedCron {
        /// The chain.
        chain: Chain,
        /// The feed ID.
        feed: String,
        /// The expression that couldn't be handled.
        expression: String,
    },
}

/// Parses a manifest's JSON text.
pub fn parse_manifest(json: &str) -> Result<RawManifest, ManifestError> {
    serde_json::from_str(json).map_err(|e| ManifestError::Invalid(e.to_string()))
}

/// Every feed in the chain's vendored manifest, in feed ID order.
pub fn load_feeds(chain: Chain) -> Result<Vec<Feed>, ManifestError> {
    feeds_from_manifest(chain, &parse_manifest(chain.vendored_manifest())?)
}

/// Every feed in `manifest`, with its schedule resolved.
pub fn feeds_from_manifest(
    chain: Chain,
    manifest: &RawManifest,
) -> Result<Vec<Feed>, ManifestError> {
    if manifest.chain.id != chain.chain_id() {
        return Err(ManifestError::WrongChain {
            chain,
            found: manifest.chain.id,
        });
    }
    let defaults = &manifest.update_triggers;
    let (Some(heartbeat_ms), Some(deviation_pct)) = (
        defaults.time_since_last_update_in_milliseconds,
        defaults.deviation_percentage,
    ) else {
        return Err(ManifestError::IncompleteDefaults(chain));
    };
    manifest
        .price_feeds
        .iter()
        .map(|(id, raw)| {
            let address = Address::parse(&raw.price_feed_address).ok_or_else(|| {
                ManifestError::InvalidAddress {
                    chain,
                    feed: id.clone(),
                }
            })?;
            let schedule = resolve_schedule(chain, id, heartbeat_ms, deviation_pct, raw)?;
            Ok(Feed {
                chain,
                id: id.clone(),
                address,
                schedule,
            })
        })
        .collect()
}

fn resolve_schedule(
    chain: Chain,
    id: &str,
    heartbeat_ms: u64,
    deviation_pct: f64,
    raw: &RawFeed,
) -> Result<Schedule, ManifestError> {
    let overrides = raw.update_triggers_overrides.clone().unwrap_or_default();
    if let Some(expressions) = overrides.cron {
        if let Some(bad) = expressions.iter().find(|e| !is_daily_cron(e)) {
            return Err(ManifestError::UnsupportedCron {
                chain,
                feed: id.to_owned(),
                expression: bad.clone(),
            });
        }
        return Ok(Schedule::Cron {
            expressions,
            period: Duration::from_secs(86_400),
        });
    }
    Ok(Schedule::Triggers {
        heartbeat: Duration::from_millis(
            overrides
                .time_since_last_update_in_milliseconds
                .unwrap_or(heartbeat_ms),
        ),
        deviation_pct: overrides.deviation_percentage.unwrap_or(deviation_pct),
    })
}

/// True for `M H * * *` with a valid minute and hour: once a day at a fixed time.
fn is_daily_cron(expression: &str) -> bool {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    let in_range = |field: &str, max: u8| field.parse::<u8>().is_ok_and(|v| v <= max);
    matches!(fields.as_slice(), [minute, hour, "*", "*", "*"] if in_range(minute, 59) && in_range(hour, 23))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(86_400);

    fn feed(chain: Chain, id: &str) -> Feed {
        load_feeds(chain)
            .unwrap()
            .into_iter()
            .find(|f| f.id == id)
            .unwrap()
    }

    fn manifest(feeds: &str) -> RawManifest {
        parse_manifest(&format!(
            r#"{{"chain":{{"name":"mainnet","id":1}},
                "updateTriggers":{{"deviationPercentage":0.5,"timeSinceLastUpdateInMilliseconds":86400000}},
                "adapterContract":"0x0000000000000000000000000000000000000001",
                "priceFeeds":{{{feeds}}}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn fr_fw_1_manifests_parse() {
        let expected = [
            (Chain::Ethereum, 95),
            (Chain::Arbitrum, 21),
            (Chain::Base, 30),
        ];
        for (chain, feeds) in expected {
            let manifest = parse_manifest(chain.vendored_manifest()).unwrap();
            assert_eq!(manifest.chain.id, chain.chain_id(), "{chain}");
            assert_eq!(manifest.price_feeds.len(), feeds, "{chain}");
            assert_eq!(load_feeds(chain).unwrap().len(), feeds, "{chain}");
        }
    }

    #[test]
    fn fr_fw_1_feed_override_replaces_the_defaults() {
        let lbtc = feed(Chain::Ethereum, "LBTC_FUNDAMENTAL");
        assert_eq!(
            lbtc.schedule,
            Schedule::Triggers {
                heartbeat: DAY,
                deviation_pct: 1.0
            }
        );
        assert_eq!(
            lbtc.address.as_str(),
            "0xb415eAA355D8440ac7eCB602D3fb67ccC1f0bc81"
        );
    }

    #[test]
    fn fr_fw_1_feed_without_override_gets_the_defaults() {
        let usd3 = feed(Chain::Ethereum, "USD3");
        assert_eq!(
            usd3.schedule,
            Schedule::Triggers {
                heartbeat: DAY,
                deviation_pct: 0.5
            }
        );
    }

    #[test]
    fn fr_fw_1_partial_override_keeps_the_other_default() {
        let raw = manifest(
            r#""A":{"priceFeedAddress":"0x0000000000000000000000000000000000000002",
                    "updateTriggersOverrides":{"timeSinceLastUpdateInMilliseconds":3600000}},
               "B":{"priceFeedAddress":"0x0000000000000000000000000000000000000003",
                    "updateTriggersOverrides":{"deviationPercentage":2}}"#,
        );
        let feeds = feeds_from_manifest(Chain::Ethereum, &raw).unwrap();
        let hour = Duration::from_secs(3_600);
        assert_eq!(
            feeds[0].schedule,
            Schedule::Triggers {
                heartbeat: hour,
                deviation_pct: 0.5
            }
        );
        assert_eq!(
            feeds[1].schedule,
            Schedule::Triggers {
                heartbeat: DAY,
                deviation_pct: 2.0
            }
        );
    }

    #[test]
    fn fr_fw_1_cron_feeds_use_a_daily_period() {
        let mzusd = feed(Chain::Ethereum, "mzUSD_FUNDAMENTAL");
        assert_eq!(
            mzusd.schedule,
            Schedule::Cron {
                expressions: vec!["0 10 * * *".to_owned()],
                period: DAY
            }
        );
        assert_eq!(mzusd.schedule.max_gap(), DAY);
        let crons = load_feeds(Chain::Ethereum)
            .unwrap()
            .into_iter()
            .filter(|f| matches!(f.schedule, Schedule::Cron { .. }))
            .count();
        assert_eq!(crons, 4);
    }

    #[test]
    fn fr_fw_1_rejects_a_non_daily_cron() {
        for expression in [
            "*/5 * * * *",
            "0 10 * * 1",
            "60 10 * * *",
            "0 24 * * *",
            "0 10 * *",
        ] {
            let raw = manifest(&format!(
                r#""X":{{"priceFeedAddress":"0x0000000000000000000000000000000000000002",
                        "updateTriggersOverrides":{{"cron":["{expression}"]}}}}"#
            ));
            assert_eq!(
                feeds_from_manifest(Chain::Ethereum, &raw).unwrap_err(),
                ManifestError::UnsupportedCron {
                    chain: Chain::Ethereum,
                    feed: "X".to_owned(),
                    expression: expression.to_owned()
                }
            );
        }
    }

    #[test]
    fn fr_fw_1_rejects_a_bad_address() {
        for address in [
            "0x123",
            "b415eAA355D8440ac7eCB602D3fb67ccC1f0bc81",
            "0xZZ15eAA355D8440ac7eCB602D3fb67ccC1f0bc81",
        ] {
            let raw = manifest(&format!(r#""X":{{"priceFeedAddress":"{address}"}}"#));
            assert_eq!(
                feeds_from_manifest(Chain::Ethereum, &raw).unwrap_err(),
                ManifestError::InvalidAddress {
                    chain: Chain::Ethereum,
                    feed: "X".to_owned()
                }
            );
        }
    }

    #[test]
    fn fr_fw_1_rejects_a_manifest_for_another_chain() {
        let raw =
            manifest(r#""X":{"priceFeedAddress":"0x0000000000000000000000000000000000000002"}"#);
        let error = feeds_from_manifest(Chain::Base, &raw).unwrap_err();
        assert_eq!(
            error,
            ManifestError::WrongChain {
                chain: Chain::Base,
                found: 1
            }
        );
        assert_eq!(error.to_string(), "manifest for base declares chain ID 1");
    }

    #[test]
    fn fr_fw_1_rejects_incomplete_defaults() {
        let mut raw = manifest("");
        raw.update_triggers.deviation_percentage = None;
        assert_eq!(
            feeds_from_manifest(Chain::Ethereum, &raw).unwrap_err(),
            ManifestError::IncompleteDefaults(Chain::Ethereum)
        );
    }

    #[test]
    fn fr_fw_1_rejects_a_manifest_without_feeds() {
        let error = parse_manifest(r#"{"chain":{"name":"x","id":1}}"#).unwrap_err();
        assert!(error.to_string().starts_with("manifest is not valid"));
    }

    #[test]
    fn chain_tags_match_the_data_contract() {
        let tags: Vec<_> = Chain::ALL.iter().map(|c| c.to_string()).collect();
        assert_eq!(tags, ["ethereum", "arbitrum", "base"]);
    }
}
