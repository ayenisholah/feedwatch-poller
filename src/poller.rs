//! One poll cycle: read every feed on every chain, take reference prices, run the checks, track
//! incidents, and write everything to InfluxDB in one request.
//!
//! Delivers NFR-1 (chains read concurrently, batches of at most 20). `main` runs [`Poller::cycle`]
//! on an interval; `health` reports on the results.

use std::collections::HashMap;
use std::time::Instant;

use futures::future::join_all;

use crate::abi::{DECIMALS, LATEST_ROUND_DATA, decode_decimals, decode_round};
use crate::checks::{IncidentTracker, Reading, assess};
use crate::influx::{
    InfluxStore, Point, feed_check, feed_round, incident, line_protocol, poll_cycle, rpc_call,
};
use crate::manifest::Feed;
use crate::reference::{Asset, ReferenceBook, SpotSource};
use crate::rpc::{Call, CallError, RpcCallStat, RpcClient, RpcTransport};

/// What one cycle did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleReport {
    /// Unix seconds when the cycle started.
    pub started_at: u64,
    /// Wall time of the cycle.
    pub duration_ms: u64,
    /// Feeds checked.
    pub feeds: usize,
    /// Feeds whose status isn't `ok`.
    pub failed_feeds: usize,
    /// Incidents open after the cycle.
    pub open_incidents: usize,
    /// True when a chain had no provider answer, or the write to InfluxDB failed.
    pub failed: bool,
}

/// One chain's feeds and RPC client, with each feed's decimals cached after the first read.
#[derive(Debug)]
pub struct ChainReader<T> {
    client: RpcClient<T>,
    feeds: Vec<Feed>,
    decimals: HashMap<String, u8>,
}

impl<T: RpcTransport> ChainReader<T> {
    /// A reader for `feeds`, which must all be on the client's chain.
    pub fn new(client: RpcClient<T>, feeds: Vec<Feed>) -> ChainReader<T> {
        ChainReader {
            client,
            feeds,
            decimals: HashMap::new(),
        }
    }

    /// Reads every feed. The flag is true when no provider answered at all.
    async fn read(&mut self) -> (Vec<(Feed, Reading)>, Vec<RpcCallStat>, bool) {
        let mut calls = Vec::new();
        for feed in &self.feeds {
            calls.push(Call {
                to: feed.address.as_str().to_owned(),
                data: LATEST_ROUND_DATA,
            });
            if !self.decimals.contains_key(&feed.id) {
                calls.push(Call {
                    to: feed.address.as_str().to_owned(),
                    data: DECIMALS,
                });
            }
        }
        let outcome = self.client.call_batch(&calls).await;
        let unreachable = outcome
            .results
            .iter()
            .all(|r| r == &Err(CallError::AllProvidersFailed));
        let mut results = outcome.results.into_iter();
        let mut readings = Vec::with_capacity(self.feeds.len());
        for feed in &self.feeds {
            let round = results.next().unwrap_or(Err(CallError::MissingResponse));
            if !self.decimals.contains_key(&feed.id)
                && let Some(Ok(hex)) = results.next()
                && let Ok(decimals) = decode_decimals(&hex)
            {
                self.decimals.insert(feed.id.clone(), decimals);
            }
            readings.push((
                feed.clone(),
                reading(round, self.decimals.get(&feed.id).copied()),
            ));
        }
        (readings, outcome.stats, unreachable)
    }
}

/// Turns a `latestRoundData()` call outcome into a reading.
pub fn reading(round: Result<String, CallError>, decimals: Option<u8>) -> Reading {
    match round {
        Ok(hex) => match (decode_round(&hex), decimals) {
            (Ok(round), Some(decimals)) => Reading::Round { round, decimals },
            (Ok(_), None) => Reading::Unavailable("decimals unknown".to_owned()),
            (Err(e), _) => Reading::Unavailable(format!("undecodable result: {e}")),
        },
        Err(CallError::Rpc { message, .. }) if message.contains("revert") => {
            Reading::Reverted(message)
        }
        Err(e) => Reading::Unavailable(e.to_string()),
    }
}

/// Everything a cycle needs, owned across cycles.
#[derive(Debug)]
pub struct Poller<T, S, I> {
    chains: Vec<ChainReader<T>>,
    references: ReferenceBook<T, S>,
    store: I,
    incidents: IncidentTracker,
}

impl<T: RpcTransport, S: SpotSource, I: InfluxStore> Poller<T, S, I> {
    /// A poller. `incidents` should be seeded from [`InfluxStore::open_incidents`].
    pub fn new(
        chains: Vec<ChainReader<T>>,
        references: ReferenceBook<T, S>,
        store: I,
        incidents: IncidentTracker,
    ) -> Self {
        Poller {
            chains,
            references,
            store,
            incidents,
        }
    }

    /// Runs one cycle at `now` (Unix seconds).
    pub async fn cycle(&mut self, now: u64) -> CycleReport {
        let started = Instant::now();
        let reads = join_all(self.chains.iter_mut().map(ChainReader::read)).await;
        let (prices, reference_stats) = self.references.prices(now).await;
        let mut points: Vec<Point> = Vec::new();
        let mut report = CycleReport {
            started_at: now,
            duration_ms: 0,
            feeds: 0,
            failed_feeds: 0,
            open_incidents: 0,
            failed: false,
        };
        for (readings, stats, unreachable) in reads {
            report.failed |= unreachable;
            points.extend(stats.iter().map(|s| rpc_call(s, now)));
            for (feed, reading) in readings {
                let reference = Asset::for_feed(&feed.id).and_then(|a| prices.get(&a));
                let assessment = assess(&feed, &reading, reference, now);
                if let (Reading::Round { round, .. }, Some(value), Some(age)) =
                    (&reading, assessment.value, assessment.age_s)
                {
                    points.push(feed_round(&feed, round, value, age, now));
                }
                let source = reference
                    .filter(|_| assessment.reference_deviation_pct.is_some())
                    .map(|r| r.source);
                points.push(feed_check(&feed, &assessment, source, now));
                points.extend(
                    self.incidents
                        .observe(&feed, &assessment, now)
                        .iter()
                        .map(|e| incident(e, now)),
                );
                report.feeds += 1;
                report.failed_feeds += usize::from(assessment.status != crate::checks::Status::Ok);
            }
        }
        points.extend(reference_stats.iter().map(|s| rpc_call(s, now)));
        report.duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        points.push(poll_cycle(
            report.duration_ms,
            report.feeds,
            report.failed_feeds,
            now,
        ));
        if let Err(error) = self.store.write(line_protocol(&points)).await {
            tracing::error!(%error, "InfluxDB write failed");
            report.failed = true;
        }
        report.open_incidents = self.incidents.open_count();
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_from_call_outcomes() {
        let round = format!("0x{:064x}{:064x}{:064x}{:064x}{:064x}", 1, 5, 9, 9, 1);
        assert!(matches!(
            reading(Ok(round.clone()), Some(8)),
            Reading::Round { decimals: 8, .. }
        ));
        assert_eq!(
            reading(Ok(round), None),
            Reading::Unavailable("decimals unknown".to_owned())
        );
        assert_eq!(
            reading(Ok("0x".to_owned()), Some(8)),
            Reading::Unavailable("undecodable result: expected 5 words, got 0 bytes".to_owned())
        );
        let revert = CallError::Rpc {
            code: 3,
            message: "execution reverted".to_owned(),
        };
        assert_eq!(
            reading(Err(revert), Some(8)),
            Reading::Reverted("execution reverted".to_owned())
        );
        let other = CallError::Rpc {
            code: -32000,
            message: "header not found".to_owned(),
        };
        assert_eq!(
            reading(Err(other), Some(8)),
            Reading::Unavailable("rpc error -32000: header not found".to_owned())
        );
        assert_eq!(
            reading(Err(CallError::AllProvidersFailed), None),
            Reading::Unavailable("every provider failed".to_owned())
        );
    }
}
