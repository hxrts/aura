#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mkdir -p artifacts/harness/browser

(
  cd crates/aura-harness/playwright-driver
  npm ci
  npm run install-browsers
)

bash scripts/check/harness-browser-install.sh

cargo build -p aura-harness --bin aura-harness -q
export AURA_HARNESS_BIN="$repo_root/target/debug/aura-harness"
export AURA_HARNESS_WEB_BUILD_PROFILE=release

bash scripts/harness/run-matrix.sh --lane web "$@"
