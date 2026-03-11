#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root/crates/aura-harness/playwright-driver"

node ./playwright_driver.mjs --selftest
