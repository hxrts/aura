#!/usr/bin/env bash
# Ensure shared intent UI flow uses canonical paths without legacy bypass shortcuts.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-shared-intent-ui-flow: $*" >&2
  exit 1
}

backend_contract="crates/aura-harness/src/backend/mod.rs"
playwright_backend="crates/aura-harness/src/backend/playwright_browser.rs"
local_backend="crates/aura-harness/src/backend/local_pty.rs"

if rg -n 'SHARED_INTENT_UI_BYPASS_ALLOWLIST|TemporaryHarnessBridgeShortcut' "$backend_contract" >/dev/null; then
  fail "legacy shared intent UI bypass allowlist machinery must be removed"
fi

if rg -n 'context_workflows::|invitation_workflows::' crates/aura-harness/src/backend >/dev/null; then
  fail "backend implementations must not call app-internal workflow shortcuts directly"
fi

cargo test -p aura-harness local_shared_intent_methods_use_semantic_harness_commands_for_shared_flows --quiet
cargo test -p aura-harness playwright_shared_intent_methods_use_semantic_bridge --quiet
cargo test -p aura-harness playwright_shared_semantic_methods_do_not_regress_to_raw_ui_driving --quiet
cargo test -p aura-harness playwright_shared_semantic_bridge_replaces_shortcut_bypasses --quiet

echo "harness shared-intent ui flow: clean"
