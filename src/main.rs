//! Entry point. Wires modules together only (feedwatch-platform `docs/ENGINEERING_STANDARDS.md`).

use feedwatch_poller::manifest::{Chain, parse_manifest};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    for chain in Chain::ALL {
        let manifest = parse_manifest(chain.vendored_manifest())?;
        tracing::info!(
            chain = chain.as_str(),
            feeds = manifest.price_feeds.len(),
            "manifest loaded"
        );
    }
    tracing::info!("scaffold only: the poll loop arrives in M1 (feat/poll-loop)");
    Ok(())
}
