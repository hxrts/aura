#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-browser-install: $*" >&2
  exit 1
}

driver_dir="crates/aura-harness/playwright-driver"
driver_script="$driver_dir/playwright_driver.mjs"

[[ -f "$driver_script" ]] || fail "missing Playwright driver script: $driver_script"
command -v node >/dev/null 2>&1 || fail "node not found in PATH"

(
  cd "$driver_dir"
  node -e "const { chromium } = require('playwright'); const p = chromium.executablePath(); if (!p) process.exit(2); process.stdout.write(p);"
) >/dev/null 2>&1 || fail "Playwright chromium is unavailable; run npm ci and npm run install-browsers in $driver_dir"

echo "harness browser install: clean"
