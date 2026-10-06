#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
web_root="$repo_root/web"

(
  cd "$web_root"
  CI=true pnpm install --frozen-lockfile
  CI=true pnpm build
)

test -f "$web_root/dist/index.html"
build_dir="$(mktemp -d "$repo_root/public/.dist-build.XXXXXX")"
stage="$build_dir/new"
backup="$build_dir/old"
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -d "$backup" && ! -e "$repo_root/public/dist" ]]; then
    mv "$backup" "$repo_root/public/dist" || status=1
  fi
  rm -rf "$build_dir"
  exit "$status"
}
trap cleanup EXIT

mkdir "$stage"
if [[ -f "$repo_root/public/dist/README.md" ]]; then
  cp "$repo_root/public/dist/README.md" "$stage/README.md"
fi
cp -a "$web_root/dist/." "$stage/"
test -f "$stage/index.html"

if [[ -e "$repo_root/public/dist" ]]; then
  mv "$repo_root/public/dist" "$backup"
fi
if ! mv "$stage" "$repo_root/public/dist"; then
  if [[ -d "$backup" ]]; then
    mv "$backup" "$repo_root/public/dist"
  fi
  exit 1
fi
test -f "$repo_root/public/dist/index.html"
rm -rf "$backup"
trap - EXIT
rm -rf "$build_dir"

touch "$repo_root/src/static_files.rs"

echo "Built Rulist frontend into $repo_root/public/dist"
