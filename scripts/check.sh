#!/usr/bin/env bash
set -euo pipefail

# Ensure Corepack does not block on interactive download prompts
export COREPACK_ENABLE_DOWNLOAD_PROMPT=0

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "=== [1/5] Checking Rust code format ==="
cargo fmt --check

echo "=== [2/5] Running Rust clippy ==="
cargo clippy --all-targets -- -D warnings

echo "=== [3/5] Checking frontend types (tsc) ==="
pnpm --prefix web lint

if [[ "${1:-}" == "--fast" ]]; then
    echo "=== Fast checks passed (skipping test suites with --fast) ==="
    exit 0
fi

echo "=== [4/5] Running frontend tests ==="
pnpm --prefix web test

echo "=== [5/5] Running Rust tests ==="
cargo test

echo "=== All checks passed! ==="
