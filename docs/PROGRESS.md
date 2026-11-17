# Progress

**Updated:** 2026-11-18

Read this first. Update it in the same commit as the work. The system-wide state is in `../feedwatch-platform/docs/PROGRESS.md`.

## Next step

1. Platform PR `feat/compose-poller`: add the `poller` service to `feedwatch-platform/compose.yaml` (`build: ../feedwatch-poller`, `env_file`, health check on `/health`).
2. Platform `feat/grafana-dashboard` (FR-FW-8), following `../feedwatch-platform/docs/plans/m1-poller.md` step 8.
3. Close M1 (step 9): a 2 h soak, findings recorded, requirements marked done.

## Done

| Date | Work |
|---|---|
| 2026-11-18 | `ci/deploy-on-merge` (NFR-3): a `Deploy` workflow calls feedwatch-platform's reusable deploy workflow after `CI` passes on `main`, so every merge reaches production (platform ADR 0008). Needs the `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` secrets from platform `scripts/aws/set-deploy-secrets.sh`. |
| 2026-11-17 | `feat/poll-loop` (NFR-1, NFR-2): `Poller::cycle` reads the chains concurrently, caching decimals, and maps reverts to `error`. It applies references, checks and incidents, and makes one write per cycle. `main` runs it on an interval with graceful shutdown, serves `/health` (503 after 3 missed intervals) and `/metrics`, and prints an EMF line per cycle. First live run: 146 feeds in about 1.5 s; USUAL is an `error` incident; the incident is reloaded on restart. |
| 2026-11-17 | `build(deny)`: ignored RUSTSEC-2026-0440 (unmaintained `astral-tokio-tar`, reached only through the testcontainers dev-dependency). |
| 2026-11-17 | `feat/influx-sink` (FR-FW-5): line protocol for all five measurements, checked by a contract test against `tests/fixtures/influx-schema.json`. That's a vendored copy of the platform contract, now v2 with `deviation_threshold_pct` optional for cron feeds; the test also checks the copy matches a sibling platform checkout. `HttpInflux` writes, and `open_incidents` reloads incidents whose last event is `open` (Flux, annotated CSV). Real InfluxDB 2.9.1 tests run through testcontainers when `CI` is set. Config gains `INFLUX_URL`, `INFLUX_TOKEN`, `INFLUX_ORG` and `INFLUX_BUCKET`. |
| 2026-11-17 | `feat/reference` (FR-FW-4): `ReferenceBook::prices(now)` reads Chainlink ETH, BTC and USDC on mainnet in one batch and uses each while within its maximum age (1 h for ETH and BTC, 24 h for USDC). Otherwise it uses Coinbase spot; with neither, there's no reference and drift is skipped. 30 s cache. `Asset::for_feed` matches only `ETH`, `BTC` and `USDC`. Addresses verified on-chain through `description()` (platform F-16 and F-17). |
| 2026-11-17 | `feat/checks` (FR-FW-3, FR-FW-4, FR-FW-5 logic): `assess` gives `stale` past heartbeat × 1.1 (cron feeds use their daily period). It gives `drift` beyond the deviation threshold after 10 minutes (trigger feeds with a reference), and `error` for reverts (such as `USUAL`) or no answer. Stale wins over drift. `IncidentTracker` produces open and close events with one ID, closes then opens on a kind change, and resumes from stored open incidents. `spread` is range over median. |
| 2026-11-17 | `feat/rpc` (FR-FW-2, NFR-1): `RpcClient::call_batch` sends batches of 20 or fewer, matches responses by `id`, retries transient failures twice, then fails over to the next provider. It keeps per-call JSON-RPC errors and records one `RpcCallStat` per request, with the host only. `Config::from_env` validates the RPC lists and the poll interval. Live: publicnode accepted batches of 100 (F-12). |
| 2026-11-17 | `feat/abi` (part of FR-FW-2): decodes `latestRoundData()` and `decimals()` results. Rejects negative answers, out-of-range values, wrong lengths and non-hex. Tested against a real ETH round recorded on 2026-11-17 (`tests/fixtures/`). |
| 2026-11-17 | `feat/manifest` (FR-FW-1): `load_feeds(chain)` returns every feed with its address and schedule. Overrides are merged field by field into the defaults. The 4 Ethereum `cron` feeds (`mzUSD_*`, `mzPRIME_*`) become a daily `Schedule::Cron` with freshness checks only. Rejects bad addresses, non-daily cron, wrong chain IDs and incomplete defaults. |
| 2026-11-16 | Scaffold: Cargo project, module stubs naming their requirements, manifests vendored at RedStone commit `c75bac13` (95/21/30 feeds), `fr_fw_1_manifests_parse`, check script, Dockerfile, CI. |

## In progress

Nothing.

## Blocked

Nothing.

## Notes

- `cargo-deny` isn't installed locally; CI installs it. Install it with `cargo install cargo-deny --locked`.
