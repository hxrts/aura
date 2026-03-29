#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
driver_dir="$repo_root/crates/aura-harness/playwright-driver"

cd "$driver_dir"
npm run typecheck

rg -q "./contracts.js" src/playwright_driver.ts \
  || { echo "harness-browser-driver-types: driver does not import typed contracts" >&2; exit 1; }
rg -q "./method_sets.js" src/playwright_driver.ts \
  || { echo "harness-browser-driver-types: driver does not import typed method sets" >&2; exit 1; }
rg -q "./driver_loader.mjs" playwright_driver.mjs \
  || { echo "harness-browser-driver-types: stable wrapper does not delegate to the driver loader" >&2; exit 1; }
rg -q "dist', 'playwright_driver.js'" driver_loader.mjs \
  || { echo "harness-browser-driver-types: stable wrapper does not load compiled TS driver" >&2; exit 1; }

echo "harness-browser-driver-types: clean"
