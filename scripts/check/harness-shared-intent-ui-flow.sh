#!/usr/bin/env bash
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

rg -q 'pub const SHARED_INTENT_UI_BYPASS_ALLOWLIST' "$backend_contract" \
  || fail "missing shared intent UI bypass allowlist"
rg -q 'TemporaryHarnessBridgeShortcut' "$backend_contract" \
  || fail "missing typed shared intent UI bypass classification"

for method in submit_create_account submit_create_home submit_create_contact_invitation; do
  rg -q "method_name: \"$method\"" "$backend_contract" \
    || fail "missing allowlisted shared-intent bypass metadata for $method"
done

if rg -n 'context_workflows::|invitation_workflows::' crates/aura-harness/src/backend >/dev/null; then
  fail "backend implementations must not call app-internal workflow shortcuts directly"
fi

cargo test -p aura-harness shared_intent_ui_bypass_allowlist_is_explicit_and_unique --quiet
cargo test -p aura-harness local_shared_intent_methods_drive_visible_tui_controls --quiet
cargo test -p aura-harness playwright_shared_intent_methods_use_visible_ui_controls --quiet
cargo test -p aura-harness playwright_shortcut_bypasses_are_allowlisted --quiet

echo "harness shared-intent ui flow: clean"
