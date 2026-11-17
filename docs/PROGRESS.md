# Progress

**Updated:** 2026-11-16

Read this first. Update it in the same commit as the work. The system-wide state is in `../feedwatch-platform/docs/PROGRESS.md`.

## Next step

Branch `feat/checks` (FR-FW-3, FR-FW-4), following `../feedwatch-platform/docs/plans/m1-poller.md` step 4. It's pure: `assess`, `IncidentTracker` and `spread`.

**New input from the live check:** a feed's `latestRoundData()` can revert. On 2026-11-17, Ethereum `USUAL` (`0x2240AE461B34CC56D654ba5FA5830A243Ca54840`) reverted with `InvalidLastUpdateDetails(bytes32,uint256,uint256,uint256)` (selector `0x4f319ffe`). Its data carries a last-update timestamp of 2026-11-05 12:01 UTC, against a 24 h heartbeat (platform fact F-15). Handle it this way:
- Give `assess` the call outcome, not just `Option<Round>`.
- A revert is `error`, with the revert reason in the incident detail.
- Decoding `InvalidLastUpdateDetails` to report it as `stale` with its timestamp is a follow-up, once the meaning of its four fields is confirmed in RedStone's contracts.

## Done

| Date | Work |
|---|---|
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
