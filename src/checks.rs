//! Pure checks: freshness against the heartbeat (FR-FW-3), missed updates against a reference
//! (FR-FW-4), incident open and close (FR-FW-5), and cross-chain spread. Time is passed in as Unix
//! seconds; no I/O here. Rules: feedwatch-platform `SPEC.md`, "Checks".

use std::collections::HashMap;

use crate::abi::Round;
use crate::manifest::{Chain, Feed, Schedule};

/// Grace on top of the heartbeat before a feed counts as stale: 10%.
pub const STALE_GRACE: f64 = 1.1;
/// A feed has this long to follow a reference move before it counts as drifting.
pub const DRIFT_GRACE_SECS: u64 = 600;

/// What reading a feed produced this cycle.
#[derive(Debug, Clone, PartialEq)]
pub enum Reading {
    /// A decoded round and the feed's decimals.
    Round {
        /// The latest round.
        round: Round,
        /// The feed's decimals.
        decimals: u8,
    },
    /// The contract reverted, with the node's reason. Ethereum `USUAL` does this when its data is
    /// too old (feedwatch-platform `docs/VERIFIED_FACTS.md`, F-15).
    Reverted(String),
    /// No answer: every provider failed, or the result couldn't be decoded.
    Unavailable(String),
}

/// A reference price for the feed's asset.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    /// Price in USD.
    pub price: f64,
    /// Where it came from: `chainlink` or `coinbase`.
    pub source: &'static str,
}

/// A feed's status, as stored in the `feed_check` measurement's `status` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// Fresh, and following the reference where there is one.
    Ok,
    /// Older than its heartbeat plus grace.
    Stale,
    /// Missed an update after a reference move beyond its deviation threshold.
    Drift,
    /// Couldn't be read: reverted, or no provider answered.
    Error,
}

impl Status {
    /// The tag value (feedwatch-platform `contracts/influx-schema.json`).
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Stale => "stale",
            Status::Drift => "drift",
            Status::Error => "error",
        }
    }
}

/// The result of checking one feed.
#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    /// The feed's status.
    pub status: Status,
    /// The price, scaled by decimals, when it could be read.
    pub value: Option<f64>,
    /// Seconds since the feed's last update, when it could be read.
    pub age_s: Option<u64>,
    /// How far the value is from the reference, in percent, when there is a reference.
    pub reference_deviation_pct: Option<f64>,
    /// Why the status isn't `ok`, for incident details.
    pub detail: Option<String>,
}

/// Checks one feed. `now` is Unix seconds.
pub fn assess(
    feed: &Feed,
    reading: &Reading,
    reference: Option<&Reference>,
    now: u64,
) -> Assessment {
    let (round, decimals) = match reading {
        Reading::Round { round, decimals } => (round, *decimals),
        Reading::Reverted(reason) => return failed(format!("reverted: {reason}")),
        Reading::Unavailable(reason) => return failed(format!("unavailable: {reason}")),
    };
    let value = round.scaled(decimals);
    let age_s = now.saturating_sub(round.updated_at);
    let max_gap = feed.schedule.max_gap().as_secs_f64();
    let deviation = reference
        .filter(|r| r.price > 0.0)
        .map(|r| (r.price - value).abs() / r.price * 100.0);
    let mut assessment = Assessment {
        status: Status::Ok,
        value: Some(value),
        age_s: Some(age_s),
        reference_deviation_pct: deviation,
        detail: None,
    };
    if age_s as f64 > max_gap * STALE_GRACE {
        assessment.status = Status::Stale;
        assessment.detail = Some(format!(
            "last update {age_s} s ago; expected at least every {max_gap} s"
        ));
    } else if let (Schedule::Triggers { deviation_pct, .. }, Some(off), Some(r)) =
        (&feed.schedule, deviation, reference)
        && off > *deviation_pct
        && age_s > DRIFT_GRACE_SECS
    {
        assessment.status = Status::Drift;
        assessment.detail = Some(format!(
            "{off:.3}% from {} ({}) for {age_s} s; threshold {deviation_pct}%",
            r.source, r.price
        ));
    }
    assessment
}

fn failed(detail: String) -> Assessment {
    Assessment {
        status: Status::Error,
        value: None,
        age_s: None,
        reference_deviation_pct: None,
        detail: Some(detail),
    }
}

/// Whether an incident event opens or closes an incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncidentEdge {
    /// The feed left `ok`, or changed to a different non-ok status.
    Open,
    /// The feed returned to `ok`, or changed to a different non-ok status.
    Close,
}

impl IncidentEdge {
    /// The `event` tag value.
    pub fn as_str(self) -> &'static str {
        match self {
            IncidentEdge::Open => "open",
            IncidentEdge::Close => "close",
        }
    }
}

/// One incident event, for the `incident` measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentEvent {
    /// The chain.
    pub chain: Chain,
    /// The feed ID.
    pub feed_id: String,
    /// The non-ok status the incident is about.
    pub kind: Status,
    /// Open or close.
    pub edge: IncidentEdge,
    /// Links an incident's open and close: `{chain}:{feed}:{opened unix seconds}`.
    pub incident_id: String,
    /// Why it opened, or empty on close.
    pub detail: String,
}

/// An incident that hasn't closed yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenIncident {
    /// Its ID.
    pub incident_id: String,
    /// The status it's about.
    pub kind: Status,
}

/// Remembers open incidents between cycles and turns status changes into events.
#[derive(Debug, Default)]
pub struct IncidentTracker {
    open: HashMap<(Chain, String), OpenIncident>,
}

impl IncidentTracker {
    /// A tracker that resumes with incidents still open in storage, so a restart doesn't
    /// open them twice (`influx::load_open_incidents`).
    pub fn with_open(
        open: impl IntoIterator<Item = (Chain, String, OpenIncident)>,
    ) -> IncidentTracker {
        IncidentTracker {
            open: open
                .into_iter()
                .map(|(chain, id, incident)| ((chain, id), incident))
                .collect(),
        }
    }

    /// The events this assessment causes: none, an open, a close, or a close then an open when
    /// the kind changes. `now` is Unix seconds.
    pub fn observe(
        &mut self,
        feed: &Feed,
        assessment: &Assessment,
        now: u64,
    ) -> Vec<IncidentEvent> {
        let key = (feed.chain, feed.id.clone());
        let mut events = Vec::new();
        if let Some(current) = self.open.get(&key) {
            if current.kind == assessment.status {
                return events;
            }
            events.push(self.event(
                feed,
                current.kind,
                IncidentEdge::Close,
                current.incident_id.clone(),
                String::new(),
            ));
            self.open.remove(&key);
        }
        if assessment.status != Status::Ok {
            let incident_id = format!("{}:{}:{now}", feed.chain, feed.id);
            let detail = assessment.detail.clone().unwrap_or_default();
            events.push(self.event(
                feed,
                assessment.status,
                IncidentEdge::Open,
                incident_id.clone(),
                detail,
            ));
            self.open.insert(
                key,
                OpenIncident {
                    incident_id,
                    kind: assessment.status,
                },
            );
        }
        events
    }

    /// Incidents currently open.
    pub fn open_count(&self) -> usize {
        self.open.len()
    }

    fn event(
        &self,
        feed: &Feed,
        kind: Status,
        edge: IncidentEdge,
        incident_id: String,
        detail: String,
    ) -> IncidentEvent {
        IncidentEvent {
            chain: feed.chain,
            feed_id: feed.id.clone(),
            kind,
            edge,
            incident_id,
            detail,
        }
    }
}

/// Cross-chain spread of one asset: `(max - min) / median × 100`, in percent. `None` with fewer
/// than two values or a zero median.
pub fn spread(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    let median = if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    };
    let (min, max) = (sorted[0], sorted[sorted.len() - 1]);
    (median != 0.0).then(|| (max - min) / median * 100.0)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::manifest::Address;

    const NOW: u64 = 1_800_000_000;
    const HOUR: u64 = 3_600;

    fn feed(schedule: Schedule) -> Feed {
        Feed {
            chain: Chain::Base,
            id: "ETH".to_owned(),
            address: Address::parse("0x0000000000000000000000000000000000000001").unwrap(),
            schedule,
        }
    }

    fn triggers(heartbeat_h: u64, deviation_pct: f64) -> Feed {
        feed(Schedule::Triggers {
            heartbeat: Duration::from_secs(heartbeat_h * HOUR),
            deviation_pct,
        })
    }

    /// A round worth `price` (8 decimals) last updated `age` seconds before NOW.
    fn reading(price: f64, age: u64) -> Reading {
        Reading::Round {
            round: Round {
                round_id: 1,
                answer: (price * 1e8) as u128,
                updated_at: NOW - age,
            },
            decimals: 8,
        }
    }

    fn chainlink(price: f64) -> Reference {
        Reference {
            price,
            source: "chainlink",
        }
    }

    #[test]
    fn fr_fw_3_stale_only_past_heartbeat_plus_grace() {
        let feed = triggers(24, 0.5);
        let limit = (24.0 * HOUR as f64 * STALE_GRACE) as u64;
        assert_eq!(
            assess(&feed, &reading(100.0, limit), None, NOW).status,
            Status::Ok
        );
        let stale = assess(&feed, &reading(100.0, limit + 1), None, NOW);
        assert_eq!(stale.status, Status::Stale);
        assert_eq!(stale.age_s, Some(limit + 1));
        assert!(
            stale
                .detail
                .unwrap()
                .contains("expected at least every 86400 s")
        );
    }

    #[test]
    fn fr_fw_3_cron_feeds_use_their_daily_period() {
        let cron = feed(Schedule::Cron {
            expressions: vec!["0 10 * * *".to_owned()],
            period: Duration::from_secs(24 * HOUR),
        });
        assert_eq!(
            assess(&cron, &reading(1.0, 27 * HOUR), None, NOW).status,
            Status::Stale
        );
        assert_eq!(
            assess(&cron, &reading(1.0, 2 * HOUR), Some(&chainlink(2.0)), NOW).status,
            Status::Ok
        );
    }

    #[test]
    fn fr_fw_4_drift_beyond_threshold_after_ten_minutes() {
        let feed = triggers(24, 0.5);
        let drift = assess(
            &feed,
            &reading(100.0, DRIFT_GRACE_SECS + 1),
            Some(&chainlink(101.0)),
            NOW,
        );
        assert_eq!(drift.status, Status::Drift);
        assert!((drift.reference_deviation_pct.unwrap() - 0.990_099).abs() < 1e-5);
        assert!(
            drift
                .detail
                .unwrap()
                .starts_with("0.990% from chainlink (101)")
        );
    }

    #[test]
    fn fr_fw_4_no_drift_within_the_grace_or_threshold() {
        let feed = triggers(24, 0.5);
        assert_eq!(
            assess(
                &feed,
                &reading(100.0, DRIFT_GRACE_SECS),
                Some(&chainlink(101.0)),
                NOW
            )
            .status,
            Status::Ok
        );
        let close = assess(&feed, &reading(100.0, HOUR), Some(&chainlink(100.4)), NOW);
        assert_eq!(close.status, Status::Ok);
        assert!(close.reference_deviation_pct.is_some());
        assert_eq!(
            assess(&feed, &reading(100.0, HOUR), Some(&chainlink(0.0)), NOW)
                .reference_deviation_pct,
            None
        );
    }

    #[test]
    fn fr_fw_3_stale_wins_over_drift() {
        let feed = triggers(1, 0.5);
        assert_eq!(
            assess(
                &feed,
                &reading(100.0, 2 * HOUR),
                Some(&chainlink(150.0)),
                NOW
            )
            .status,
            Status::Stale
        );
    }

    #[test]
    fn unreadable_feeds_are_errors_with_the_reason() {
        let feed = triggers(24, 0.5);
        let reverted = assess(
            &feed,
            &Reading::Reverted("execution reverted".to_owned()),
            None,
            NOW,
        );
        assert_eq!(reverted.status, Status::Error);
        assert_eq!(
            reverted.detail.as_deref(),
            Some("reverted: execution reverted")
        );
        assert_eq!(reverted.value, None);
        let down = assess(
            &feed,
            &Reading::Unavailable("every provider failed".to_owned()),
            None,
            NOW,
        );
        assert_eq!(
            down.detail.as_deref(),
            Some("unavailable: every provider failed")
        );
    }

    #[test]
    fn a_future_update_time_counts_as_fresh() {
        let feed = triggers(24, 0.5);
        let ahead = Reading::Round {
            round: Round {
                round_id: 1,
                answer: 1,
                updated_at: NOW + 30,
            },
            decimals: 0,
        };
        assert_eq!(assess(&feed, &ahead, None, NOW).age_s, Some(0));
    }

    #[test]
    fn fr_fw_5_incident_opens_and_closes_with_one_id() {
        let feed = triggers(24, 0.5);
        let mut tracker = IncidentTracker::default();
        let stale = assess(&feed, &reading(1.0, 48 * HOUR), None, NOW);
        let opened = tracker.observe(&feed, &stale, NOW);
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].edge, IncidentEdge::Open);
        assert_eq!(opened[0].incident_id, format!("base:ETH:{NOW}"));
        assert_eq!(
            tracker.observe(&feed, &stale, NOW + 60),
            vec![],
            "still stale: nothing new"
        );
        assert_eq!(tracker.open_count(), 1);
        let closed = tracker.observe(
            &feed,
            &assess(&feed, &reading(1.0, 1), None, NOW + 120),
            NOW + 120,
        );
        assert_eq!(closed.len(), 1);
        assert_eq!(
            (
                closed[0].edge,
                closed[0].kind,
                closed[0].incident_id.as_str()
            ),
            (
                IncidentEdge::Close,
                Status::Stale,
                opened[0].incident_id.as_str()
            )
        );
        assert_eq!(tracker.open_count(), 0);
    }

    #[test]
    fn fr_fw_5_kind_change_closes_then_opens() {
        let feed = triggers(24, 0.5);
        let mut tracker = IncidentTracker::with_open([(
            Chain::Base,
            "ETH".to_owned(),
            OpenIncident {
                incident_id: "base:ETH:1".to_owned(),
                kind: Status::Stale,
            },
        )]);
        let error = assess(&feed, &Reading::Reverted("x".to_owned()), None, NOW);
        let events = tracker.observe(&feed, &error, NOW);
        let edges: Vec<_> = events
            .iter()
            .map(|e| (e.edge.as_str(), e.kind.as_str()))
            .collect();
        assert_eq!(edges, [("close", "stale"), ("open", "error")]);
        assert_eq!(events[0].incident_id, "base:ETH:1");
        assert_eq!(events[1].detail, "reverted: x");
    }

    #[test]
    fn status_tags_match_the_contract() {
        let tags: Vec<_> = [Status::Ok, Status::Stale, Status::Drift, Status::Error]
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(tags, ["ok", "stale", "drift", "error"]);
    }

    #[test]
    fn spread_is_range_over_median() {
        assert_eq!(spread(&[100.0]), None);
        assert_eq!(spread(&[0.0, 0.0]), None);
        assert!((spread(&[99.0, 100.0, 101.0]).unwrap() - 2.0).abs() < 1e-9);
        assert!((spread(&[100.0, 102.0]).unwrap() - 1.980_198).abs() < 1e-5);
    }
}
