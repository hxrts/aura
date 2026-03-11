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

frontend_hits="$(rg -l 'AURA_HARNESS_MODE' crates/aura-terminal/src crates/aura-web/src | grep -v 'crates/aura-terminal/src/tui/screens/app/shell/events.rs' || true)"
if [[ -n "$frontend_hits" ]]; then
  fail "frontend product modules must not branch on AURA_HARNESS_MODE: $frontend_hits"
fi

if rg -q 'reset_harness_bootstrap_storage_once' crates/aura-web/src/main.rs; then
  fail "web frontend may not carry harness-only bootstrap reset shortcuts"
fi

cargo test -p aura-app harness_mode_allowlist_is_scoped_to_non_semantic_categories --quiet
cargo test -p aura-app connectivity_check_is_harness_mode_neutral --quiet
cargo test -p aura-terminal invitation_dispatch_uses_product_callbacks_without_harness_shortcuts --quiet

echo "harness mode allowlist: clean"
