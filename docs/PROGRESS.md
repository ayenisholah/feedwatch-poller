# Progress

**Updated:** 2026-11-16

Read this first. Update it in the same commit as the work. The system-wide state is in `../feedwatch-platform/docs/PROGRESS.md`.

## Next step

Branch `feat/manifest` (FR-FW-1). In `src/manifest.rs`, add a `Feed` type (chain, feed ID, address, heartbeat, deviation threshold) and `load_feeds(chain)`, which merges `update_triggers_overrides` into the defaults. Test it with a feed that has an override (Ethereum `LBTC_FUNDAMENTAL`: 1% deviation, 24 h heartbeat) and one that doesn't (`USD3`). Validate addresses as 0x plus 40 hex characters.

After that: `feat/abi`, then `feat/rpc`, `feat/checks`, `feat/reference`, `feat/influx-sink` and `feat/poll-loop` (feedwatch-platform `docs/ROADMAP.md`, M1).

## Done

| Date | Work |
|---|---|
| 2026-11-16 | Scaffold: Cargo project, module stubs naming their requirements, manifests vendored at RedStone commit `c75bac13` (95/21/30 feeds), `fr_fw_1_manifests_parse`, check script, Dockerfile, CI. |

## In progress

Nothing.

## Blocked

Nothing.

## Notes

- `cargo-deny` isn't installed locally; CI installs it. Install it with `cargo install cargo-deny --locked`.
