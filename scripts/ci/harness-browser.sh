#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mkdir -p artifacts/harness/browser

(
  cd crates/aura-harness/playwright-driver
  npm ci
  npm run build
  npm run install-browsers
)

bash scripts/check/harness-browser-install.sh
export AURA_HARNESS_WEB_BUILD_PROFILE=release

cargo run -p aura-harness --bin aura-harness -- run \
  --config configs/harness/browser-loopback.toml \
  --scenario scenarios/harness/semantic-observation-browser-smoke.toml \
  --artifacts-dir artifacts/harness/browser
