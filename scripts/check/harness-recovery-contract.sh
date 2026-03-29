#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-recovery-contract: $*" >&2
  exit 1
}

cargo test -p aura-harness registered_recoveries_cover_all_paths --quiet

rg -q 'export const RECOVERY_METHODS' crates/aura-harness/playwright-driver/src/method_sets.ts \
  || fail "missing registered recovery metadata"
rg -q "'recover_ui_state'" crates/aura-harness/playwright-driver/src/method_sets.ts \
  || fail "recover_ui_state must remain registered as an explicit recovery method"

echo "harness recovery contract: clean"
