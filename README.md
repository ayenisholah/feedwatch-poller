# feedwatch-poller

The Feedwatch worker. It reads every RedStone push feed on Ethereum, Arbitrum and Base over public JSON-RPC, runs the freshness and missed-update checks, and writes results to InfluxDB.

Part of [Feedwatch](../feedwatch-platform/SPEC.md). System docs, the data contract and the roadmap live in `feedwatch-platform`.

> **Status:** Scaffolded (M0). Loads and parses the vendored manifests; no RPC reads yet. See [progress](docs/PROGRESS.md).

## Run

```sh
cp .env.example .env
cargo run                # logs the feed count per chain (scaffold)
```

## Check

```sh
./scripts/check.sh       # fmt, clippy -D warnings, tests, coverage ≥95% (cargo-llvm-cov), cargo-deny
docker build -t feedwatch-poller .
```

## Licence

MIT
