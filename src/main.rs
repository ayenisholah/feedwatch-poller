//! Entry point. Wires modules together only (feedwatch-platform `docs/ENGINEERING_STANDARDS.md`).

use feedwatch_poller::manifest::{Chain, Schedule, load_feeds};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    for chain in Chain::ALL {
        let feeds = load_feeds(chain)?;
        let cron = feeds
            .iter()
            .filter(|f| matches!(f.schedule, Schedule::Cron { .. }))
            .count();
        tracing::info!(
            chain = chain.as_str(),
            feeds = feeds.len(),
            cron,
            "feeds loaded"
        );
    }
    tracing::info!("scaffold only: the poll loop arrives in M1 (feat/poll-loop)");
    Ok(())
}
