//! Health and metrics for the running poller (NFR-2), and the CloudWatch embedded metric format
//! (EMF) line logged once per cycle (FR-FW-9 in M4).
//!
//! Pure: the HTTP routes in `main` call these with the shared state and the current time.

use serde_json::json;

use crate::poller::CycleReport;

/// What health and metrics report on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HealthState {
    /// The last cycle, successful or not.
    pub last: Option<CycleReport>,
    /// When the last successful cycle started (Unix seconds).
    pub last_success: Option<u64>,
}

impl HealthState {
    /// Records a finished cycle.
    pub fn record(&mut self, report: CycleReport) {
        if !report.failed {
            self.last_success = Some(report.started_at);
        }
        self.last = Some(report);
    }
}

/// `/health`: 200 while the last successful cycle is less than three intervals old, else 503.
pub fn health(state: &HealthState, now: u64, interval_secs: u64) -> (u16, String) {
    let healthy = state
        .last_success
        .is_some_and(|t| now.saturating_sub(t) < 3 * interval_secs);
    let body = json!({
        "status": if healthy { "ok" } else { "unhealthy" },
        "lastSuccessAt": state.last_success,
        "lastCycleFailed": state.last.as_ref().map(|r| r.failed),
    });
    (if healthy { 200 } else { 503 }, body.to_string())
}

/// `/metrics`: the last cycle in Prometheus text format.
pub fn metrics(state: &HealthState) -> String {
    let Some(last) = &state.last else {
        return "# no cycle has finished yet\n".to_owned();
    };
    let rows = [
        (
            "feedwatch_poll_duration_ms",
            "Duration of the last poll cycle.",
            last.duration_ms,
        ),
        (
            "feedwatch_feeds",
            "Feeds checked in the last cycle.",
            last.feeds as u64,
        ),
        (
            "feedwatch_failed_feeds",
            "Feeds not ok in the last cycle.",
            last.failed_feeds as u64,
        ),
        (
            "feedwatch_open_incidents",
            "Incidents open after the last cycle.",
            last.open_incidents as u64,
        ),
        (
            "feedwatch_poll_cycle_failed",
            "1 if the last cycle failed.",
            u64::from(last.failed),
        ),
        (
            "feedwatch_last_success_timestamp_seconds",
            "Start of the last successful cycle.",
            state.last_success.unwrap_or(0),
        ),
    ];
    rows.iter()
        .map(|(name, help, value)| {
            format!("# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}\n")
        })
        .collect()
}

/// One EMF log line for a cycle. CloudWatch Logs turns it into the `Feedwatch/PollCycleFailed`
/// metric the M4 alarm watches (feedwatch-platform `docs/DEPLOYMENT.md`).
pub fn emf_line(report: &CycleReport) -> String {
    json!({
        "_aws": {
            "Timestamp": report.started_at.saturating_mul(1_000),
            "CloudWatchMetrics": [{
                "Namespace": "Feedwatch",
                "Dimensions": [[]],
                "Metrics": [
                    {"Name": "PollCycleFailed", "Unit": "Count"},
                    {"Name": "PollDurationMs", "Unit": "Milliseconds"},
                    {"Name": "FailedFeeds", "Unit": "Count"}
                ]
            }]
        },
        "PollCycleFailed": u8::from(report.failed),
        "PollDurationMs": report.duration_ms,
        "FailedFeeds": report.failed_feeds,
        "OpenIncidents": report.open_incidents,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn report(started_at: u64, failed: bool) -> CycleReport {
        CycleReport {
            started_at,
            duration_ms: 900,
            feeds: 146,
            failed_feeds: 1,
            open_incidents: 1,
            failed,
        }
    }

    #[test]
    fn nfr_2_unhealthy_until_a_cycle_succeeds() {
        let state = HealthState::default();
        assert_eq!(health(&state, 100, 60).0, 503);
        assert_eq!(metrics(&state), "# no cycle has finished yet\n");
    }

    #[test]
    fn nfr_2_healthy_for_three_intervals_after_success() {
        let mut state = HealthState::default();
        state.record(report(1_000, false));
        let (status, body) = health(&state, 1_179, 60);
        assert_eq!(status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["lastSuccessAt"],
            1_000
        );
        state.record(report(1_060, true));
        assert_eq!(
            state.last_success,
            Some(1_000),
            "a failed cycle doesn't count"
        );
        assert_eq!(health(&state, 1_180, 60).0, 503);
    }

    #[test]
    fn nfr_2_metrics_in_prometheus_format() {
        let mut state = HealthState::default();
        state.record(report(1_000, false));
        let text = metrics(&state);
        assert!(text.contains("# TYPE feedwatch_feeds gauge\nfeedwatch_feeds 146\n"));
        assert!(text.contains("feedwatch_poll_cycle_failed 0\n"));
        assert!(text.contains("feedwatch_last_success_timestamp_seconds 1000\n"));
    }

    #[test]
    fn emf_line_carries_poll_cycle_failed() {
        let line: Value = serde_json::from_str(&emf_line(&report(1_000, true))).unwrap();
        assert_eq!(line["PollCycleFailed"], 1);
        assert_eq!(line["_aws"]["Timestamp"], 1_000_000);
        assert_eq!(
            line["_aws"]["CloudWatchMetrics"][0]["Namespace"],
            "Feedwatch"
        );
        assert_eq!(
            line["_aws"]["CloudWatchMetrics"][0]["Metrics"][0]["Name"],
            "PollCycleFailed"
        );
    }
}
