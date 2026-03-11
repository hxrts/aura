#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-mode-allowlist: $*" >&2
  exit 1
}

ui_contract="crates/aura-app/src/ui_contract.rs"

rg -q 'pub const HARNESS_MODE_ALLOWLIST' "$ui_contract" \
  || fail "missing harness-mode allowlist metadata"
rg -q 'enum HarnessModeChangeKind' "$ui_contract" \
  || fail "missing harness-mode change kind metadata"

cargo test -p aura-app harness_mode_allowlist_is_scoped_to_non_semantic_categories --quiet
cargo test -p aura-app connectivity_check_is_harness_mode_neutral --quiet

echo "harness mode allowlist: clean"
