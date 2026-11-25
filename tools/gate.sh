#!/usr/bin/env bash
# The gate, in the order CI runs it: formatting, lints, tests, then a
# release build. .github/workflows/ci.yml runs exactly these steps.
#
# The one thing this script cannot do on Linux is *link and run* the
# Windows binaries, so LNK2019-class errors and Windows-only test
# failures only appear in CI. Everything else — rustfmt, clippy with
# -D warnings, the whole test suite, the release profile — is here.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked

echo "gate: ok"
