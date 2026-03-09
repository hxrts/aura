#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-ui-state-evented: $*" >&2
  exit 1
}

rg -q 'wait_for_ui_state' crates/aura-harness/playwright-driver/playwright_driver.mjs \
  || fail "playwright driver is missing wait_for_ui_state RPC"

rg -q 'wait_for_ui_snapshot_event' crates/aura-harness/src/backend/playwright_browser.rs \
  || fail "playwright browser backend is missing event-driven UiSnapshot wait support"

rg -q 'wait_for_ui_snapshot_event' crates/aura-harness/src/coordinator.rs \
  || fail "coordinator is missing event-driven UiSnapshot wait plumbing"

rg -q 'wait_for_ui_snapshot_event' crates/aura-harness/src/tool_api.rs \
  || fail "tool API is missing event-driven UiSnapshot wait plumbing"

rg -q 'wait_for_ui_snapshot_event' crates/aura-harness/src/executor.rs \
  || fail "executor semantic waits are not using event-driven UiSnapshot waits"

if ! rg -n 'fn wait_for_semantic_state' crates/aura-harness/src/executor.rs \
  | rg -q 'wait_for_semantic_state'; then
  fail "executor is missing semantic wait helper"
fi

echo "harness ui-state evented policy: clean"
