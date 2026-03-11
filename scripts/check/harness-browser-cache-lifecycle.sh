#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-browser-cache-lifecycle: $*" >&2
  exit 1
}

rg -q 'pub const BROWSER_CACHE_BOUNDARIES' crates/aura-app/src/ui_contract.rs \
  || fail "missing browser cache lifecycle metadata"

for reason in session_start authority_switch device_import storage_reset navigation_recovery
do
  rg -q "$reason" crates/aura-app/src/ui_contract.rs \
    || fail "missing browser cache lifecycle reason: $reason"
done

echo "harness browser cache lifecycle: clean"
