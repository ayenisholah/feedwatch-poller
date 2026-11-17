# Progress

**Updated:** 2026-11-16

Read this first. Update it in the same commit as the work. The system-wide state is in `../feedwatch-platform/docs/PROGRESS.md`.

## Next step

Branch `feat/abi` (part of FR-FW-2), following `../feedwatch-platform/docs/plans/m1-poller.md` step 2. In `src/abi.rs`, decode `latestRoundData()` and `decimals()` results (int256 two's complement to i128). Reject short, odd-length, non-hex and negative answers. Use the real 2026-11-16 call results as fixtures.

## Done

| Date | Work |
|---|---|
| 2026-11-17 | `feat/manifest` (FR-FW-1): `load_feeds(chain)` returns every feed with its address and schedule. Overrides are merged field by field into the defaults. The 4 Ethereum `cron` feeds (`mzUSD_*`, `mzPRIME_*`) become a daily `Schedule::Cron` with freshness checks only. Rejects bad addresses, non-daily cron, wrong chain IDs and incomplete defaults. |
| 2026-11-16 | Scaffold: Cargo project, module stubs naming their requirements, manifests vendored at RedStone commit `c75bac13` (95/21/30 feeds), `fr_fw_1_manifests_parse`, check script, Dockerfile, CI. |

## In progress

Nothing.

## Blocked

Nothing.

## Notes

- `cargo-deny` isn't installed locally; CI installs it. Install it with `cargo install cargo-deny --locked`.
