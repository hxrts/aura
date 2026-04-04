#!/usr/bin/env bash
# Run the Playwright driver self-test for observation contract compliance.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bash "$repo_root/scripts/check/browser-toolchain.sh"
cd "$repo_root/crates/aura-harness/playwright-driver"

node ./playwright_driver.mjs --selftest
