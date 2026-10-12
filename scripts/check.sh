#!/usr/bin/env bash
set -euo pipefail

# Ensure Corepack does not block on interactive download prompts
export COREPACK_ENABLE_DOWNLOAD_PROMPT=0

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

if ! node --experimental-strip-types -e "" >/dev/null 2>&1; then
    echo "=== Error: Current Node.js does not support --experimental-strip-types ==="
    echo "Please ensure official Node.js >= 22.6 is installed and in PATH."
    exit 1
fi

echo "=== [1/5] Checking Rust code format ==="
cargo fmt --check

echo "=== [2/5] Running Rust clippy ==="
cargo clippy --all-targets -- -D warnings

echo "=== [3/5] Checking frontend types (tsc) ==="
pnpm --prefix web lint

if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    modified_frontend_files=$(git diff HEAD --name-only --diff-filter=d | grep -E '^web/src/.*\.(js|ts|tsx|jsx|css|json)$' || true)
    if [[ -n "$modified_frontend_files" ]]; then
        echo "=== [3.5/5] Checking modified frontend code formatting (prettier) ==="
        echo "$modified_frontend_files" | sed 's|^web/||' | (cd web && xargs pnpm exec prettier --check)
    fi
fi

if [[ "${1:-}" == "--fast" ]]; then
    echo "=== Fast checks passed (skipping test suites with --fast) ==="
    exit 0
fi

echo "=== [4/5] Running frontend tests ==="
pnpm --prefix web test

echo "=== [5/5] Running Rust tests ==="
cargo test

echo "=== All checks passed! ==="
