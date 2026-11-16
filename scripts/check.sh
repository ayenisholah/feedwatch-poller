#!/usr/bin/env bash
# Every gate CI runs, in the same order. Run before opening a pull request.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
# main.rs only wires modules together (docs/ENGINEERING_STANDARDS.md), so coverage is measured on the library.
cargo llvm-cov --locked --fail-under-lines 95 --ignore-filename-regex 'src/main\.rs$' --summary-only
if command -v cargo-deny >/dev/null 2>&1; then
  cargo deny check
else
  echo "cargo-deny not installed (cargo install cargo-deny --locked); CI runs it"
fi
