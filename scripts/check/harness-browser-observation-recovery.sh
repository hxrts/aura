#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness browser observation recovery: $*" >&2
  exit 1
}

driver="crates/aura-harness/playwright-driver/playwright_driver.mjs"
observation_module="crates/aura-harness/playwright-driver/src/observation.ts"
ui_state_body="$(awk '/async function uiState\\(params\\)/,/^}/ {print}' "$driver")"

printf '%s\n' "$ui_state_body" | rg -q 'readStructuredUiStateWithNavigationRecovery|resetObservationState\(' \
  && fail "ui_state may not perform implicit browser recovery"

rg -q 'const RECOVERY_METHODS = new Set' "$driver" \
  || fail "driver must declare explicit recovery methods"

rg -q "case 'recover_ui_state'" "$driver" \
  || fail "driver must expose explicit recover_ui_state"

if rg -q 'recover|retry|fallback' "$observation_module"; then
  fail "browser observation module must stay passive and recovery-free"
fi

for forbidden in \
  'click_button js_fallback_' \
  'click_button css fallback_key' \
  'click_button nav_label_first' \
  'fill_input fallback_done' \
  'locator_click_force:' \
  'key_press_dom_fallback_' \
  'selectorToFallbackLabel'
do
  if rg -q "$forbidden" "$driver"; then
    fail "legacy implicit browser fallback remains in driver: $forbidden"
  fi
done

node crates/aura-harness/playwright-driver/playwright_driver.mjs --selftest
# Note: observation_endpoints_are_side_effect_free is run by harness-observation-surface.sh

echo "harness browser observation recovery: clean"
