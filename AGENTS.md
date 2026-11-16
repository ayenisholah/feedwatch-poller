# feedwatch-poller: agent instructions

The Feedwatch worker. It reads every RedStone push feed on Ethereum, Arbitrum and Base over public JSON-RPC, runs the freshness and missed-update checks, and writes results to InfluxDB. Rust, edition 2024, toolchain pinned in `rust-toolchain.toml`.

## Start of every session

1. Read [`docs/PROGRESS.md`](docs/PROGRESS.md). Its "Next step" says exactly what to do.
2. System rules live in the sibling repo `../feedwatch-platform`: `AGENTS.md`, `docs/ENGINEERING_STANDARDS.md`, `docs/TEST_STRATEGY.md`, `docs/REQUIREMENTS.md`, `docs/DATA_CONTRACT.md` and `docs/VERIFIED_FACTS.md`. Follow them here.

## Commands

```sh
./scripts/check.sh                     # every gate CI runs
cargo test                             # tests only
cargo llvm-cov --open                  # coverage report
cargo run                              # run locally (copy .env.example to .env first)
```

## Rules that matter most here

- Each module's top doc comment names the requirement it delivers. Keep it accurate.
- Pure logic does no I/O; inject time, HTTP and storage through traits, with fakes in tests.
- No `unwrap` or `expect` outside tests (clippy denies them). Use `thiserror` errors, and `anyhow` only in `main`.
- Test names start with the requirement ID, such as `fr_fw_3_stale_past_heartbeat_grace`.
- `scripts/sync-manifests.sh` refreshes `manifests/` from RedStone (needs `gh` and `jq`). Do it in its own pull request.
- Branch and pull request for every change. Commits are Conventional Commits with **no attribution lines**.
- Update `docs/PROGRESS.md` in the same commit as the work.
