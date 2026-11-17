//! Configuration from the environment (see `.env.example`), validated once at start.
//!
//! `feat/rpc` adds the RPC provider lists and the poll interval. `feat/influx-sink` adds InfluxDB.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::manifest::Chain;

/// Validated poller configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// JSON-RPC provider URLs per chain, in preference order. Every chain has at least one.
    pub rpc: BTreeMap<Chain, Vec<String>>,
    /// Time between poll cycles.
    pub poll_interval: Duration,
}

/// A configuration problem, naming the variable at fault.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// A required variable is unset or empty.
    #[error("{0} is not set")]
    Missing(String),
    /// A variable is set but its value is unusable.
    #[error("{name} is invalid: {reason}")]
    Invalid {
        /// The variable.
        name: String,
        /// What's wrong with it.
        reason: &'static str,
    },
}

/// The environment variable that lists a chain's providers.
pub fn rpc_variable(chain: Chain) -> String {
    format!("FEEDWATCH_RPC_{}", chain.as_str().to_uppercase())
}

impl Config {
    /// Reads the configuration through `get`, which returns a variable's value if set.
    /// `main` passes `std::env::var`; tests pass a map.
    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Result<Config, ConfigError> {
        let mut rpc = BTreeMap::new();
        for chain in Chain::ALL {
            let name = rpc_variable(chain);
            let value = get(&name)
                .filter(|v| !v.trim().is_empty())
                .ok_or(ConfigError::Missing(name.clone()))?;
            let urls: Vec<String> = value
                .split(',')
                .map(|u| u.trim().to_owned())
                .filter(|u| !u.is_empty())
                .collect();
            if urls.iter().any(|u| {
                reqwest::Url::parse(u).map_or(true, |p| !matches!(p.scheme(), "http" | "https"))
            }) {
                return Err(ConfigError::Invalid {
                    name,
                    reason: "every entry must be an http(s) URL",
                });
            }
            rpc.insert(chain, urls);
        }
        let poll_interval = match get("FEEDWATCH_POLL_INTERVAL_SECS") {
            None => Duration::from_secs(60),
            Some(v) => match v.trim().parse::<u64>() {
                Ok(secs) if (10..=3_600).contains(&secs) => Duration::from_secs(secs),
                _ => {
                    return Err(ConfigError::Invalid {
                        name: "FEEDWATCH_POLL_INTERVAL_SECS".to_owned(),
                        reason: "must be whole seconds from 10 to 3600",
                    });
                }
            },
        };
        Ok(Config { rpc, poll_interval })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    const ALL_RPC: [(&str, &str); 3] = [
        (
            "FEEDWATCH_RPC_ETHEREUM",
            "https://a.example, https://b.example",
        ),
        ("FEEDWATCH_RPC_ARBITRUM", "https://c.example"),
        ("FEEDWATCH_RPC_BASE", "http://127.0.0.1:8545"),
    ];

    #[test]
    fn reads_providers_in_order_and_defaults_the_interval() {
        let config = Config::from_env(env(&ALL_RPC)).unwrap();
        assert_eq!(
            config.rpc[&Chain::Ethereum],
            ["https://a.example", "https://b.example"]
        );
        assert_eq!(config.rpc[&Chain::Base], ["http://127.0.0.1:8545"]);
        assert_eq!(config.poll_interval, Duration::from_secs(60));
    }

    #[test]
    fn reads_the_poll_interval() {
        let mut pairs = ALL_RPC.to_vec();
        pairs.push(("FEEDWATCH_POLL_INTERVAL_SECS", "30"));
        assert_eq!(
            Config::from_env(env(&pairs)).unwrap().poll_interval,
            Duration::from_secs(30)
        );
    }

    #[test]
    fn names_the_missing_variable() {
        let error = Config::from_env(env(&ALL_RPC[..2])).unwrap_err();
        assert_eq!(error, ConfigError::Missing("FEEDWATCH_RPC_BASE".to_owned()));
        assert_eq!(error.to_string(), "FEEDWATCH_RPC_BASE is not set");
    }

    #[test]
    fn rejects_bad_values() {
        let mut bad_url = ALL_RPC.to_vec();
        bad_url[1] = ("FEEDWATCH_RPC_ARBITRUM", "ftp://c.example");
        assert!(
            matches!(Config::from_env(env(&bad_url)), Err(ConfigError::Invalid { name, .. }) if name == "FEEDWATCH_RPC_ARBITRUM")
        );
        for interval in ["5", "abc", "4000"] {
            let mut pairs = ALL_RPC.to_vec();
            pairs.push(("FEEDWATCH_POLL_INTERVAL_SECS", interval));
            let error = Config::from_env(env(&pairs)).unwrap_err();
            assert_eq!(
                error.to_string(),
                "FEEDWATCH_POLL_INTERVAL_SECS is invalid: must be whole seconds from 10 to 3600"
            );
        }
    }
}
