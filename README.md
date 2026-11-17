# feedwatch-poller

[![CI](https://github.com/ayenisholah/feedwatch-poller/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/ayenisholah/feedwatch-poller/actions/workflows/ci.yml) [![Licence: MIT](https://img.shields.io/badge/licence-MIT-blue)](LICENSE) [![Rust 1.96](https://img.shields.io/badge/rust-1.96-orange)](rust-toolchain.toml)

The Feedwatch worker. It reads every RedStone push feed on Ethereum, Arbitrum and Base over public JSON-RPC, runs the freshness and missed-update checks, and writes results to InfluxDB.

Part of [Feedwatch](https://github.com/ayenisholah/feedwatch-platform/blob/main/SPEC.md). System docs, the data contract and the roadmap live in `feedwatch-platform`.

> **Status:** M1 in progress. Polls all 146 feeds every 60 s and writes results to InfluxDB. The Grafana dashboard and a 2-hour soak run are still to come. See [progress](docs/PROGRESS.md).

## Run

```sh
cp .env.example .env            # set INFLUX_TOKEN to the platform repo's .env value
(cd ../feedwatch-platform && docker compose up -d)   # InfluxDB and Grafana
set -a; . ./.env; set +a
cargo run --release             # one cycle every FEEDWATCH_POLL_INTERVAL_SECS
curl localhost:8081/health      # 200 while cycles succeed
curl localhost:8081/metrics     # last cycle, Prometheus text
```

Each cycle prints one CloudWatch EMF JSON line (`PollCycleFailed`, `PollDurationMs`, `FailedFeeds`) to stdout, next to the structured logs.

## First live run (2026-11-17)

- All 146 feeds checked in about 1.5 s.
- Ethereum `USUAL` reverts on every read (`InvalidLastUpdateDetails`; its last update was 2026-11-05), so it shows as `error` with an open incident.
- ETH, BTC and USDC on all three chains were within 0.3% of the Chainlink reference.
- Restarting the poller reloaded the open incident instead of opening a second one.

## Check

```sh
./scripts/check.sh       # fmt, clippy -D warnings, tests, coverage ≥95% (cargo-llvm-cov), cargo-deny
docker build -t feedwatch-poller .
```

## Feedwatch repositories

| Repo                                                                    | What it is                                                                         |
| ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| [feedwatch-platform](https://github.com/ayenisholah/feedwatch-platform) | Specification, requirements, ADRs, data contract, Compose, Grafana, AWS deployment |
| [feedwatch-poller](https://github.com/ayenisholah/feedwatch-poller)     | Rust worker: reads every feed, runs the checks, writes to InfluxDB                 |
| [feedwatch-api](https://github.com/ayenisholah/feedwatch-api)           | Rust (Axum) read API over InfluxDB                                                 |
| [feedwatch-web](https://github.com/ayenisholah/feedwatch-web)           | Next.js static client in the Frox design system                                    |

## Licence

MIT
