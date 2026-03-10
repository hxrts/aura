#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "shared-flow-policy: $*" >&2
  exit 1
}

bash scripts/check/harness-core-scenario-mechanics.sh
bash scripts/check/harness-ui-state-evented.sh
bash scripts/check/ui-parity-contract.sh
# Inventory and converted shared-scenario contract
bash scripts/check/harness-scenario-inventory.sh
bash scripts/check/harness-shared-scenario-contract.sh
bash scripts/check/harness-scenario-legality.sh

cargo test -p aura-app shared_flow_support_contract_is_consistent --quiet
cargo test -p aura-app shared_intent_contract_accepts_intents --quiet
cargo test -p aura-app shared_intent_contract_rejects_ui_actions --quiet
cargo test -p aura-harness --lib browser_driver_maps_shared_controls_to_selectors --quiet
cargo test -p aura-harness --lib browser_driver_maps_shared_fields_to_selectors --quiet
cargo test -p aura-harness --lib browser_driver_maps_navigation_items_to_controls --quiet

rg -q 'pub enum SharedFlowId' crates/aura-app/src/ui_contract.rs \
  || fail "missing SharedFlowId contract"
rg -q 'pub const SHARED_FLOW_SUPPORT' crates/aura-app/src/ui_contract.rs \
  || fail "missing SHARED_FLOW_SUPPORT declarations"
rg -q 'ThemeAppearance' crates/aura-app/src/ui_contract.rs \
  || fail "missing explicit theme appearance exception"

for required_id in \
  'aura-app-root' \
  'aura-modal-region' \
  'aura-toast-region' \
  'aura-modal-confirm-button' \
  'aura-modal-cancel-button'
do
  rg -q "$required_id" crates/aura-app/src/ui_contract.rs crates/aura-ui/src crates/aura-web/src \
    || fail "missing required shared-flow id: $required_id"
done

echo "shared flow policy: clean"
