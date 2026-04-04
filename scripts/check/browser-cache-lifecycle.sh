#!/usr/bin/env bash
# Verify browser cache lifecycle boundaries are declared in the UI contract.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-browser-cache-lifecycle: $*" >&2
  exit 1
}

ui_contract_files=(
  crates/aura-app/src/ui_contract.rs
  crates/aura-app/src/ui_contract/*.rs
)

rg -q 'pub const BROWSER_CACHE_BOUNDARIES' "${ui_contract_files[@]}" \
  || fail "missing browser cache lifecycle metadata"

for reason in session_start authority_switch device_import storage_reset navigation_recovery
do
  rg -q "$reason" "${ui_contract_files[@]}" \
    || fail "missing browser cache lifecycle reason: $reason"
done

echo "harness browser cache lifecycle: clean"
