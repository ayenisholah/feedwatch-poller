//! InfluxDB 2 sink: line protocol for the measurements in feedwatch-platform
//! `contracts/influx-schema.json` (version 2), an HTTP writer, and the startup query that reloads
//! open incidents so a restart doesn't open them twice.
//!
//! Delivers FR-FW-5. Line protocol is built by pure functions; HTTP is behind [`InfluxStore`].

use std::future::Future;
use std::time::Duration;

use serde_json::json;

use crate::abi::Round;
use crate::checks::{Assessment, IncidentEvent, OpenIncident, Status};
use crate::manifest::{Chain, Feed, Schedule};
use crate::rpc::RpcCallStat;

/// The contract version these points follow.
pub const CONTRACT_VERSION: u64 = 2;

/// A field value.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// A float, written as is. Non-finite floats are dropped.
    Float(f64),
    /// An integer, written with an `i` suffix.
    Int(i64),
    /// A string, quoted and escaped.
    Str(String),
    /// A boolean.
    Bool(bool),
}

/// One InfluxDB point.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    /// Measurement name.
    pub measurement: &'static str,
    /// Tags, in the order written.
    pub tags: Vec<(&'static str, String)>,
    /// Fields, in the order written. At least one must be writable.
    pub fields: Vec<(&'static str, FieldValue)>,
    /// Unix seconds.
    pub time: u64,
}

fn int(value: u64) -> FieldValue {
    FieldValue::Int(i64::try_from(value).unwrap_or(i64::MAX))
}

/// A `feed_round` point for a decoded round.
pub fn feed_round(feed: &Feed, round: &Round, value: f64, age_s: u64, now: u64) -> Point {
    Point {
        measurement: "feed_round",
        tags: vec![
            ("chain", feed.chain.to_string()),
            ("feed_id", feed.id.clone()),
        ],
        fields: vec![
            ("value", FieldValue::Float(value)),
            ("answer_raw", FieldValue::Str(round.answer_raw())),
            ("round_id", FieldValue::Str(round.round_id.to_string())),
            ("updated_at", int(round.updated_at)),
            ("age_s", int(age_s)),
        ],
        time: now,
    }
}

/// A `feed_check` point. `reference_source` is set when the drift check had a reference.
pub fn feed_check(
    feed: &Feed,
    assessment: &Assessment,
    reference_source: Option<&str>,
    now: u64,
) -> Point {
    let mut fields = vec![("heartbeat_s", int(feed.schedule.max_gap().as_secs()))];
    if let Schedule::Triggers { deviation_pct, .. } = feed.schedule {
        fields.push(("deviation_threshold_pct", FieldValue::Float(deviation_pct)));
    }
    if let Some(off) = assessment.reference_deviation_pct {
        fields.push(("reference_deviation_pct", FieldValue::Float(off)));
    }
    if let Some(source) = reference_source {
        fields.push(("reference_source", FieldValue::Str(source.to_owned())));
    }
    Point {
        measurement: "feed_check",
        tags: vec![
            ("chain", feed.chain.to_string()),
            ("feed_id", feed.id.clone()),
            ("status", assessment.status.as_str().to_owned()),
        ],
        fields,
        time: now,
    }
}

/// An `rpc_call` point.
pub fn rpc_call(stat: &RpcCallStat, now: u64) -> Point {
    Point {
        measurement: "rpc_call",
        tags: vec![
            ("chain", stat.chain.to_string()),
            ("provider", stat.provider.clone()),
        ],
        fields: vec![
            ("latency_ms", int(stat.latency_ms)),
            ("ok", FieldValue::Bool(stat.ok)),
            ("calls", int(stat.calls as u64)),
        ],
        time: now,
    }
}

/// An `incident` point.
pub fn incident(event: &IncidentEvent, now: u64) -> Point {
    Point {
        measurement: "incident",
        tags: vec![
            ("chain", event.chain.to_string()),
            ("feed_id", event.feed_id.clone()),
            ("kind", event.kind.as_str().to_owned()),
            ("event", event.edge.as_str().to_owned()),
        ],
        fields: vec![
            ("incident_id", FieldValue::Str(event.incident_id.clone())),
            ("detail", FieldValue::Str(event.detail.clone())),
        ],
        time: now,
    }
}

/// A `poll_cycle` point.
pub fn poll_cycle(duration_ms: u64, feeds: usize, failed_feeds: usize, now: u64) -> Point {
    Point {
        measurement: "poll_cycle",
        tags: vec![],
        fields: vec![
            ("duration_ms", int(duration_ms)),
            ("feeds", int(feeds as u64)),
            ("failed_feeds", int(failed_feeds as u64)),
        ],
        time: now,
    }
}

/// Points as InfluxDB line protocol, one per line, seconds precision. A point with no writable
/// field is skipped, since InfluxDB rejects it.
pub fn line_protocol(points: &[Point]) -> String {
    points
        .iter()
        .filter_map(line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn line(point: &Point) -> Option<String> {
    let fields: Vec<String> = point
        .fields
        .iter()
        .filter_map(|(key, value)| {
            let text = match value {
                FieldValue::Float(f) if f.is_finite() => format!("{f:?}"),
                FieldValue::Float(_) => return None,
                FieldValue::Int(i) => format!("{i}i"),
                FieldValue::Str(s) => {
                    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
                }
                FieldValue::Bool(b) => b.to_string(),
            };
            Some(format!("{}={text}", escape_key(key)))
        })
        .collect();
    if fields.is_empty() {
        return None;
    }
    let mut head = escape(point.measurement, &[',', ' ']);
    for (key, value) in &point.tags {
        if !value.is_empty() {
            head.push_str(&format!(",{}={}", escape_key(key), escape_key(value)));
        }
    }
    Some(format!("{head} {} {}", fields.join(","), point.time))
}

fn escape_key(text: &str) -> String {
    escape(text, &[',', '=', ' '])
}

fn escape(text: &str, special: &[char]) -> String {
    text.chars()
        .fold(String::with_capacity(text.len()), |mut out, c| {
            if special.contains(&c) {
                out.push('\\');
            }
            out.push(c);
            out
        })
}

/// Where the poller stores points and reads open incidents back. Implemented over HTTP by
/// [`HttpInflux`].
pub trait InfluxStore: Send + Sync {
    /// Writes line protocol.
    fn write(&self, lines: String) -> impl Future<Output = Result<(), String>> + Send;
    /// Incidents whose last event is `open`, within the bucket's 30-day retention.
    fn open_incidents(
        &self,
    ) -> impl Future<Output = Result<Vec<(Chain, String, OpenIncident)>, String>> + Send;
}

/// InfluxDB 2 connection settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InfluxConfig {
    /// Base URL, such as `http://influxdb:8086`.
    pub url: String,
    /// Organisation.
    pub org: String,
    /// Bucket.
    pub bucket: String,
    /// API token.
    pub token: String,
}

/// [`InfluxStore`] over the InfluxDB 2 HTTP API.
#[derive(Debug, Clone)]
pub struct HttpInflux {
    client: reqwest::Client,
    config: InfluxConfig,
}

impl HttpInflux {
    /// A client with a per-request timeout.
    pub fn new(config: InfluxConfig, timeout: Duration) -> Result<HttpInflux, String> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(HttpInflux { client, config })
    }

    fn auth(&self) -> String {
        format!("Token {}", self.config.token)
    }
}

/// The Flux query behind [`InfluxStore::open_incidents`]: the latest event per incident ID,
/// kept when it's an open.
pub fn open_incidents_query(bucket: &str) -> String {
    format!(
        r#"from(bucket: "{bucket}")
  |> range(start: -30d)
  |> filter(fn: (r) => r._measurement == "incident" and r._field == "incident_id")
  |> group(columns: ["_value"])
  |> sort(columns: ["_time"])
  |> last()
  |> group()
  |> filter(fn: (r) => r.event == "open")
  |> keep(columns: ["chain", "feed_id", "kind", "_value"])"#
    )
}

impl InfluxStore for HttpInflux {
    async fn write(&self, lines: String) -> Result<(), String> {
        let url = format!("{}/api/v2/write", self.config.url);
        let response = self
            .client
            .post(url)
            .query(&[
                ("org", &self.config.org),
                ("bucket", &self.config.bucket),
                ("precision", &"s".to_owned()),
            ])
            .header(reqwest::header::AUTHORIZATION, self.auth())
            .header(reqwest::header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(lines)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        match response.status().as_u16() {
            204 => Ok(()),
            status => Err(format!(
                "write failed: HTTP {status}: {}",
                response.text().await.unwrap_or_default()
            )),
        }
    }

    async fn open_incidents(&self) -> Result<Vec<(Chain, String, OpenIncident)>, String> {
        let url = format!("{}/api/v2/query", self.config.url);
        let response = self
            .client
            .post(url)
            .query(&[("org", &self.config.org)])
            .header(reqwest::header::AUTHORIZATION, self.auth())
            .header(reqwest::header::ACCEPT, "application/csv")
            .json(&json!({"query": open_incidents_query(&self.config.bucket), "type": "flux"}))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("query failed: HTTP {}", response.status().as_u16()));
        }
        Ok(parse_open_incidents(
            &response.text().await.map_err(|e| e.to_string())?,
        ))
    }
}

/// Reads the query's annotated CSV. Rows with an unknown chain or kind are skipped.
pub fn parse_open_incidents(csv: &str) -> Vec<(Chain, String, OpenIncident)> {
    let mut header: Option<Vec<&str>> = None;
    let mut found = Vec::new();
    for row in csv.lines().map(str::trim_end) {
        if row.is_empty() || row.starts_with('#') {
            header = if row.is_empty() { None } else { header };
            continue;
        }
        let cells: Vec<&str> = row.split(',').collect();
        let Some(columns) = &header else {
            header = Some(cells);
            continue;
        };
        let cell = |name: &str| {
            columns
                .iter()
                .position(|c| *c == name)
                .and_then(|i| cells.get(i).copied())
        };
        let chain = Chain::ALL
            .into_iter()
            .find(|c| Some(c.as_str()) == cell("chain"));
        let kind = [Status::Stale, Status::Drift, Status::Error]
            .into_iter()
            .find(|k| Some(k.as_str()) == cell("kind"));
        if let (Some(chain), Some(feed), Some(kind), Some(id)) =
            (chain, cell("feed_id"), kind, cell("_value"))
        {
            found.push((
                chain,
                feed.to_owned(),
                OpenIncident {
                    incident_id: id.to_owned(),
                    kind,
                },
            ));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::IncidentEdge;
    use crate::manifest::Address;

    fn feed(schedule: Schedule) -> Feed {
        Feed {
            chain: Chain::Ethereum,
            id: "pzETH_FUNDAMENTAL".to_owned(),
            address: Address::parse("0x0000000000000000000000000000000000000001").unwrap(),
            schedule,
        }
    }

    fn triggers() -> Schedule {
        Schedule::Triggers {
            heartbeat: Duration::from_secs(86_400),
            deviation_pct: 0.5,
        }
    }

    fn assessment(status: Status, off: Option<f64>) -> Assessment {
        Assessment {
            status,
            value: Some(1.0),
            age_s: Some(5),
            reference_deviation_pct: off,
            detail: None,
        }
    }

    #[test]
    fn fr_fw_5_writes_a_feed_round() {
        let round = Round {
            round_id: 3,
            answer: 180_633_950_235,
            updated_at: 1_794_907_199,
        };
        let line = line_protocol(&[feed_round(
            &feed(triggers()),
            &round,
            1_806.339_502_35,
            61,
            1_794_907_260,
        )]);
        assert_eq!(
            line,
            "feed_round,chain=ethereum,feed_id=pzETH_FUNDAMENTAL value=1806.33950235,answer_raw=\"180633950235\",round_id=\"3\",updated_at=1794907199i,age_s=61i 1794907260"
        );
    }

    #[test]
    fn fr_fw_5_writes_a_feed_check_with_optional_fields() {
        let with_reference = feed_check(
            &feed(triggers()),
            &assessment(Status::Drift, Some(0.75)),
            Some("chainlink"),
            10,
        );
        assert_eq!(
            line_protocol(&[with_reference]),
            "feed_check,chain=ethereum,feed_id=pzETH_FUNDAMENTAL,status=drift heartbeat_s=86400i,deviation_threshold_pct=0.5,reference_deviation_pct=0.75,reference_source=\"chainlink\" 10"
        );
        let cron = Schedule::Cron {
            expressions: vec!["0 10 * * *".to_owned()],
            period: Duration::from_secs(86_400),
        };
        assert_eq!(
            line_protocol(&[feed_check(
                &feed(cron),
                &assessment(Status::Ok, None),
                None,
                10
            )]),
            "feed_check,chain=ethereum,feed_id=pzETH_FUNDAMENTAL,status=ok heartbeat_s=86400i 10"
        );
    }

    #[test]
    fn fr_fw_5_writes_rpc_incident_and_cycle_points() {
        let stat = RpcCallStat {
            chain: Chain::Base,
            provider: "base-rpc.publicnode.com".to_owned(),
            latency_ms: 120,
            ok: true,
            calls: 20,
        };
        let event = IncidentEvent {
            chain: Chain::Ethereum,
            feed_id: "USUAL".to_owned(),
            kind: Status::Error,
            edge: IncidentEdge::Open,
            incident_id: "ethereum:USUAL:10".to_owned(),
            detail: "reverted: \"execution reverted\" \\ data".to_owned(),
        };
        assert_eq!(
            line_protocol(&[rpc_call(&stat, 10), incident(&event, 10), poll_cycle(1_234, 146, 1, 10)]),
            [
                "rpc_call,chain=base,provider=base-rpc.publicnode.com latency_ms=120i,ok=true,calls=20i 10",
                "incident,chain=ethereum,feed_id=USUAL,kind=error,event=open incident_id=\"ethereum:USUAL:10\",detail=\"reverted: \\\"execution reverted\\\" \\\\ data\" 10",
                "poll_cycle duration_ms=1234i,feeds=146i,failed_feeds=1i 10",
            ]
            .join("\n")
        );
    }

    #[test]
    fn escapes_tags_and_drops_unwritable_values() {
        let point = Point {
            measurement: "m",
            tags: vec![("k", "a b,c=d".to_owned()), ("empty", String::new())],
            fields: vec![
                ("x", FieldValue::Float(f64::NAN)),
                ("y", FieldValue::Int(1)),
            ],
            time: 1,
        };
        assert_eq!(line_protocol(&[point]), "m,k=a\\ b\\,c\\=d y=1i 1");
        let nothing = Point {
            measurement: "m",
            tags: vec![],
            fields: vec![("x", FieldValue::Float(f64::INFINITY))],
            time: 1,
        };
        assert_eq!(line_protocol(&[nothing]), "");
    }

    #[test]
    fn fr_fw_5_reads_open_incidents_from_annotated_csv() {
        let csv = "#group,false,false,true,true,true,false\r\n\
#datatype,string,long,string,string,string,string\r\n\
,result,table,chain,feed_id,kind,_value\r\n\
,_result,0,ethereum,USUAL,error,ethereum:USUAL:1794907260\r\n\
,_result,0,mars,X,error,mars:X:1\r\n\
,_result,0,base,ETH,odd,base:ETH:1\r\n\
\r\n\
,result,table,chain,feed_id,kind,_value\r\n\
,_result,1,base,BTC,stale,base:BTC:5\r\n";
        let open = parse_open_incidents(csv);
        assert_eq!(open.len(), 2);
        assert_eq!(
            open[0],
            (
                Chain::Ethereum,
                "USUAL".to_owned(),
                OpenIncident {
                    incident_id: "ethereum:USUAL:1794907260".to_owned(),
                    kind: Status::Error
                }
            )
        );
        assert_eq!(open[1].2.kind, Status::Stale);
        assert!(parse_open_incidents("").is_empty());
    }

    #[test]
    fn query_names_the_bucket() {
        assert!(open_incidents_query("feedwatch").starts_with("from(bucket: \"feedwatch\")"));
    }
}
