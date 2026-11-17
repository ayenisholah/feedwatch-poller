//! Reference prices for ETH, BTC and USDC, for the missed-update check (FR-FW-4).
//!
//! Chainlink on Ethereum mainnet is preferred while its last update is within the pair's maximum
//! age; otherwise the Coinbase spot price is used. Results are cached for 30 s. Addresses and
//! behaviour: feedwatch-platform `docs/VERIFIED_FACTS.md`, F-6, F-7, F-16 and F-17.

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use serde_json::Value;

use crate::abi::{DECIMALS, LATEST_ROUND_DATA, decode_decimals, decode_round};
use crate::checks::Reference;
use crate::rpc::{Call, RpcCallStat, RpcClient, RpcTransport};

/// How long fetched prices are reused.
pub const CACHE_SECS: u64 = 30;

/// An asset Feedwatch has a reference price for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Asset {
    /// Ether.
    Eth,
    /// Bitcoin.
    Btc,
    /// USD Coin.
    Usdc,
}

impl Asset {
    /// Every asset with a reference.
    pub const ALL: [Asset; 3] = [Asset::Eth, Asset::Btc, Asset::Usdc];

    /// The asset whose price a RedStone feed ID quotes, if Feedwatch has a reference for it.
    /// Only the plain USD feeds match: `ETH`, `BTC`, `USDC`.
    pub fn for_feed(feed_id: &str) -> Option<Asset> {
        match feed_id {
            "ETH" => Some(Asset::Eth),
            "BTC" => Some(Asset::Btc),
            "USDC" => Some(Asset::Usdc),
            _ => None,
        }
    }

    /// Chainlink's `<asset> / USD` feed on Ethereum mainnet.
    pub fn chainlink_feed(self) -> &'static str {
        match self {
            Asset::Eth => "0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419",
            Asset::Btc => "0xF4030086522a5bEEa4988F8cA5B36dbC97BeE88c",
            Asset::Usdc => "0x8fFfFfd4AfB6115b954Bd326cbe7B4BA576818f6",
        }
    }

    /// Oldest Chainlink answer still used. ETH and BTC update at least hourly; USDC moves little
    /// and was seen 11 h old on 2026-11-17, so it gets a day.
    pub fn chainlink_max_age(self) -> Duration {
        match self {
            Asset::Eth | Asset::Btc => Duration::from_secs(3_600),
            Asset::Usdc => Duration::from_secs(86_400),
        }
    }

    /// The Coinbase currency pair.
    pub fn coinbase_pair(self) -> &'static str {
        match self {
            Asset::Eth => "ETH-USD",
            Asset::Btc => "BTC-USD",
            Asset::Usdc => "USDC-USD",
        }
    }
}

/// A spot price source. Implemented over the Coinbase public API by [`CoinbaseSpot`].
pub trait SpotSource: Send + Sync {
    /// The asset's USD spot price.
    fn spot(&self, asset: Asset) -> impl Future<Output = Result<f64, String>> + Send;
}

/// Coinbase's public spot price endpoint, which needs no key (F-7).
#[derive(Debug, Clone)]
pub struct CoinbaseSpot {
    client: reqwest::Client,
    base_url: String,
}

impl CoinbaseSpot {
    /// A client for `base_url`, normally `https://api.coinbase.com`.
    pub fn new(base_url: impl Into<String>, timeout: Duration) -> Result<CoinbaseSpot, String> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(concat!("feedwatch-poller/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(CoinbaseSpot {
            client,
            base_url: base_url.into(),
        })
    }
}

impl SpotSource for CoinbaseSpot {
    async fn spot(&self, asset: Asset) -> Result<f64, String> {
        let url = format!("{}/v2/prices/{}/spot", self.base_url, asset.coinbase_pair());
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status().as_u16()));
        }
        let body: Value = response.json().await.map_err(|e| e.to_string())?;
        parse_coinbase(&body)
    }
}

fn parse_coinbase(body: &Value) -> Result<f64, String> {
    body.pointer("/data/amount")
        .and_then(Value::as_str)
        .and_then(|a| a.parse::<f64>().ok())
        .filter(|p| p.is_finite() && *p > 0.0)
        .ok_or_else(|| "no usable data.amount".to_owned())
}

/// Reference prices with a 30 s cache.
#[derive(Debug)]
pub struct ReferenceBook<T, S> {
    chainlink: RpcClient<T>,
    spot: S,
    cached: Option<(u64, HashMap<Asset, Reference>)>,
}

impl<T: RpcTransport, S: SpotSource> ReferenceBook<T, S> {
    /// `chainlink` must be an Ethereum mainnet client.
    pub fn new(chainlink: RpcClient<T>, spot: S) -> ReferenceBook<T, S> {
        ReferenceBook {
            chainlink,
            spot,
            cached: None,
        }
    }

    /// Prices at `now` (Unix seconds), and the RPC requests made to get them. An asset is missing
    /// when neither source gave a usable price; the drift check then skips it this cycle.
    pub async fn prices(&mut self, now: u64) -> (HashMap<Asset, Reference>, Vec<RpcCallStat>) {
        if let Some((fetched, prices)) = &self.cached
            && now.saturating_sub(*fetched) < CACHE_SECS
        {
            return (prices.clone(), Vec::new());
        }
        let (chainlink, stats) = self.chainlink_prices(now).await;
        let mut prices = HashMap::new();
        for asset in Asset::ALL {
            if let Some(price) = chainlink.get(&asset) {
                prices.insert(
                    asset,
                    Reference {
                        price: *price,
                        source: "chainlink",
                    },
                );
            } else if let Ok(price) = self.spot.spot(asset).await {
                prices.insert(
                    asset,
                    Reference {
                        price,
                        source: "coinbase",
                    },
                );
            }
        }
        self.cached = Some((now, prices.clone()));
        (prices, stats)
    }

    /// Chainlink prices that decoded and are within each asset's maximum age.
    async fn chainlink_prices(&self, now: u64) -> (HashMap<Asset, f64>, Vec<RpcCallStat>) {
        let calls: Vec<Call> = Asset::ALL
            .iter()
            .flat_map(|a| {
                [LATEST_ROUND_DATA, DECIMALS].map(|data| Call {
                    to: a.chainlink_feed().to_owned(),
                    data,
                })
            })
            .collect();
        let outcome = self.chainlink.call_batch(&calls).await;
        let mut prices = HashMap::new();
        for (asset, pair) in Asset::ALL.iter().zip(outcome.results.chunks(2)) {
            let (Some(Ok(round_hex)), Some(Ok(decimals_hex))) = (pair.first(), pair.get(1)) else {
                continue;
            };
            let (Ok(round), Ok(decimals)) =
                (decode_round(round_hex), decode_decimals(decimals_hex))
            else {
                continue;
            };
            if now.saturating_sub(round.updated_at) <= asset.chainlink_max_age().as_secs() {
                prices.insert(*asset, round.scaled(decimals));
            }
        }
        (prices, outcome.stats)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_plain_usd_feeds_have_a_reference() {
        assert_eq!(Asset::for_feed("ETH"), Some(Asset::Eth));
        assert_eq!(Asset::for_feed("USDC"), Some(Asset::Usdc));
        assert_eq!(Asset::for_feed("wstETH"), None);
        assert_eq!(Asset::for_feed("eth"), None);
    }

    #[test]
    fn usdc_tolerates_an_older_chainlink_answer() {
        assert_eq!(Asset::Eth.chainlink_max_age(), Duration::from_secs(3_600));
        assert_eq!(Asset::Usdc.chainlink_max_age(), Duration::from_secs(86_400));
        assert_eq!(Asset::Btc.coinbase_pair(), "BTC-USD");
    }

    #[test]
    fn parses_a_coinbase_spot_body() {
        assert_eq!(
            parse_coinbase(&json!({"data": {"amount": "1803.79", "base": "ETH"}})),
            Ok(1803.79)
        );
        assert!(parse_coinbase(&json!({"data": {"amount": "-1"}})).is_err());
        assert!(parse_coinbase(&json!({"data": {"amount": 5}})).is_err());
        assert!(parse_coinbase(&json!({})).is_err());
    }
}
