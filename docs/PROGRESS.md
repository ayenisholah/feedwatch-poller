# Progress

**Updated:** 2026-11-16

Read this first. Update it in the same commit as the work. The system-wide state is in `../feedwatch-platform/docs/PROGRESS.md`.

## Next step

Branch `feat/rpc` (FR-FW-2, NFR-1), following `../feedwatch-platform/docs/plans/m1-poller.md` step 3:
- `RpcTransport` trait plus a reqwest implementation (rustls, 5 s timeout);
- `RpcClient::call_batch` with chunks of 20 or fewer, responses matched by `id`, two retries with backoff, then failover to the next provider;
- `RpcCallStat` per request;
- `config.rs` reads the RPC lists and the poll interval.

Tests use wiremock. Afterwards, measure the largest publicnode batch by hand and record it as platform fact F-12.

## Done

| Date | Work |
|---|---|
| 2026-11-17 | `feat/abi` (part of FR-FW-2): decodes `latestRoundData()` and `decimals()` results. Rejects negative answers, out-of-range values, wrong lengths and non-hex. Tested against a real ETH round recorded on 2026-11-17 (`tests/fixtures/`). |
| 2026-11-17 | `feat/manifest` (FR-FW-1): `load_feeds(chain)` returns every feed with its address and schedule. Overrides are merged field by field into the defaults. The 4 Ethereum `cron` feeds (`mzUSD_*`, `mzPRIME_*`) become a daily `Schedule::Cron` with freshness checks only. Rejects bad addresses, non-daily cron, wrong chain IDs and incomplete defaults. |
| 2026-11-16 | Scaffold: Cargo project, module stubs naming their requirements, manifests vendored at RedStone commit `c75bac13` (95/21/30 feeds), `fr_fw_1_manifests_parse`, check script, Dockerfile, CI. |

## In progress

Nothing.

## Blocked

Nothing.

## Notes

- `cargo-deny` isn't installed locally; CI installs it. Install it with `cargo install cargo-deny --locked`.
