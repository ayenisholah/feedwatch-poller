//! Entry point. Wires modules together only (feedwatch-platform `docs/ENGINEERING_STANDARDS.md`).

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use feedwatch_poller::checks::IncidentTracker;
use feedwatch_poller::config::Config;
use feedwatch_poller::health::{HealthState, emf_line, health, metrics};
use feedwatch_poller::influx::{HttpInflux, InfluxStore};
use feedwatch_poller::manifest::{Chain, load_feeds};
use feedwatch_poller::poller::{ChainReader, Poller};
use feedwatch_poller::reference::{CoinbaseSpot, ReferenceBook};
use feedwatch_poller::rpc::{HttpTransport, RpcClient};

type Shared = (Arc<Mutex<HealthState>>, u64);

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::from_env(|name| std::env::var(name).ok())?;
    let transport = HttpTransport::new(Duration::from_secs(5))?;
    let rpc = |chain: Chain| RpcClient::new(transport.clone(), chain, config.rpc[&chain].clone());

    let mut chains = Vec::new();
    for chain in Chain::ALL {
        chains.push(ChainReader::new(rpc(chain), load_feeds(chain)?));
    }
    let spot = CoinbaseSpot::new("https://api.coinbase.com", Duration::from_secs(5))
        .map_err(anyhow::Error::msg)?;
    let store = HttpInflux::new(config.influx.clone(), Duration::from_secs(10))
        .map_err(anyhow::Error::msg)?;
    let open = store.open_incidents().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "couldn't reload open incidents; starting with none");
        Vec::new()
    });
    tracing::info!(open = open.len(), "open incidents reloaded");
    let mut poller = Poller::new(
        chains,
        ReferenceBook::new(rpc(Chain::Ethereum), spot),
        store,
        IncidentTracker::with_open(open),
    );

    let state = Arc::new(Mutex::new(HealthState::default()));
    let interval = config.poll_interval.as_secs();
    let addr = std::env::var("FEEDWATCH_HEALTH_ADDR").unwrap_or_else(|_| "0.0.0.0:8081".to_owned());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let app = axum::Router::new()
        .route("/health", get(health_route))
        .route("/metrics", get(metrics_route))
        .with_state((state.clone(), interval));
    tokio::spawn(async move { axum::serve(listener, app).await });
    tracing::info!(%addr, interval, "polling");

    let mut ticker = tokio::time::interval(config.poll_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Listen for the stop signal once, so one that arrives mid-cycle isn't missed.
    let stop = shutdown();
    tokio::pin!(stop);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let report = poller.cycle(now()).await;
                println!("{}", emf_line(&report));
                tracing::info!(feeds = report.feeds, failed_feeds = report.failed_feeds, duration_ms = report.duration_ms, failed = report.failed, "cycle");
                if let Ok(mut s) = state.lock() { s.record(report); }
            }
            _ = &mut stop => { tracing::info!("shutting down"); return Ok(()); }
        }
    }
}

async fn health_route(State((state, interval)): State<Shared>) -> (StatusCode, String) {
    let snapshot = state.lock().map(|s| s.clone()).unwrap_or_default();
    let (code, body) = health(&snapshot, now(), interval);
    (
        StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        body,
    )
}

async fn metrics_route(State((state, _)): State<Shared>) -> String {
    state.lock().map(|s| metrics(&s)).unwrap_or_default()
}

async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(term) => term,
                Err(_) => return std::future::pending().await,
            };
        tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
}
