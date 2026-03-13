#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "shared-flow-policy: $*" >&2
  exit 1
}

bash scripts/check/harness-core-scenario-mechanics.sh
bash scripts/check/harness-governance-wrappers.sh
bash scripts/check/harness-ui-state-evented.sh
bash scripts/check/ui-parity-contract.sh
# Inventory and converted shared-scenario contract
bash scripts/check/harness-scenario-inventory.sh
bash scripts/check/harness-shared-scenario-contract.sh
bash scripts/check/harness-scenario-legality.sh
bash scripts/check/harness-legacy-shared-quarantine.sh
bash scripts/check/harness-scenario-config-boundary.sh
bash scripts/check/harness-scenario-canonical-model.sh
bash scripts/check/harness-shared-flow-metadata.sh
bash scripts/check/harness-command-plane-boundary.sh
bash scripts/check/harness-trace-determinism.sh
bash scripts/check/harness-observation-determinism.sh
bash scripts/check/harness-observation-surface.sh
bash scripts/check/harness-row-index-contract.sh
bash scripts/check/harness-action-preconditions.sh
bash scripts/check/harness-mode-allowlist.sh
bash scripts/check/harness-render-convergence.sh
bash scripts/check/harness-focus-selection-contract.sh
bash scripts/check/harness-revision-contract.sh
bash scripts/check/harness-recovery-ownership.sh
bash scripts/check/harness-recovery-contract.sh
bash scripts/check/harness-wait-contract.sh
bash scripts/check/harness-semantic-primitive-contract.sh
bash scripts/check/harness-shared-intent-ui-flow.sh
bash scripts/check/harness-backend-contract.sh
bash scripts/check/harness-shared-raw-ui-quarantine.sh
bash scripts/check/harness-raw-backend-quarantine.sh
bash scripts/check/harness-settings-surface-contract.sh
bash scripts/check/harness-tui-semantic-snapshot.sh
bash scripts/check/harness-tui-selection-contract.sh
bash scripts/check/harness-tui-product-path.sh
bash scripts/check/harness-onboarding-publication.sh
bash scripts/check/harness-runtime-events-authoritative.sh
bash scripts/check/harness-browser-observation-recovery.sh
bash scripts/check/harness-shared-semantic-dedup.sh
bash scripts/check/harness-tui-observation-channel.sh
bash scripts/check/harness-export-override-policy.sh
bash scripts/check/harness-onboarding-contract.sh
bash scripts/check/harness-bridge-contract.sh
bash scripts/check/harness-browser-cache-owner.sh
bash scripts/check/harness-browser-cache-lifecycle.sh
bash scripts/check/harness-browser-observation-contract.sh
bash scripts/check/harness-browser-driver-types.sh

cargo test -p aura-app shared_flow_support_contract_is_consistent --quiet
cargo test -p aura-app shared_intent_contract_accepts_intents --quiet
cargo test -p aura-app shared_intent_contract_rejects_ui_actions --quiet
cargo test -p aura-app shared_intent_contract_rejects_row_index_item_ids --quiet
cargo test -p aura-app every_intent_kind_has_a_matching_contract --quiet
cargo test -p aura-app every_intent_kind_declares_barrier_metadata --quiet
cargo test -p aura-app declared_post_operation_convergence_contracts_are_explicit --quiet
cargo test -p aura-app snapshot_invariants_reject_placeholder_ids --quiet
cargo test -p aura-app snapshot_invariants_reject_override_backed_ids --quiet
cargo test -p aura-app snapshot_invariants_reject_row_index_ids --quiet
cargo test -p aura-app snapshot_invariants_reject_inferred_runtime_events --quiet
cargo test -p aura-app snapshot_invariants_reject_contradictory_focus_and_modal_state --quiet
cargo test -p aura-app projection_revision_detects_stale_snapshots_by_revision --quiet
cargo test -p aura-app onboarding_is_declared_in_the_shared_snapshot_model --quiet
cargo test -p aura-app ui_snapshot_parity_detects_focus_semantic_drift --quiet
cargo test -p aura-app browser_harness_bridge_contract_is_versioned_and_complete --quiet
cargo test -p aura-app browser_harness_bridge_read_methods_are_declared_deterministic --quiet
cargo test -p aura-app browser_observation_surface_contract_is_versioned_and_read_only --quiet
cargo test -p aura-app tui_observation_surface_contract_is_versioned_and_read_only --quiet
cargo test -p aura-app harness_shell_structure_accepts_exactly_one_app_shell --quiet
cargo test -p aura-app harness_shell_structure_accepts_single_onboarding_shell --quiet
cargo test -p aura-app harness_shell_structure_rejects_duplicate_or_ambiguous_roots --quiet
cargo test -p aura-app observation_surface_methods_do_not_overlap_action_surface --quiet
cargo test -p aura-app harness_mode_allowlist_is_scoped_to_non_semantic_categories --quiet
cargo test -p aura-app connectivity_check_is_harness_mode_neutral --quiet
cargo test -p aura-app frontend_execution_boundaries_are_defined_and_exist --quiet
cargo test -p aura-app ui_snapshot_parity_reports_undeclared_drift --quiet
cargo test -p aura-app render_convergence_accepts_matching_snapshot_and_heartbeat --quiet
cargo test -p aura-app render_convergence_rejects_semantic_state_published_ahead_of_renderer --quiet
cargo test -p aura-harness --lib browser_driver_maps_shared_controls_to_selectors --quiet
cargo test -p aura-harness --lib browser_driver_maps_shared_fields_to_selectors --quiet
cargo test -p aura-harness --lib browser_driver_maps_navigation_items_to_controls --quiet
cargo test -p aura-harness observation_endpoints_are_side_effect_free --quiet
cargo test -p aura-harness wait_contract_refs_cover_all_parity_wait_kinds --quiet
cargo test -p aura-harness shared_intent_waits_bind_only_to_declared_barriers --quiet
cargo test -p aura-harness action_preconditions_fail_diagnostically_before_issue --quiet
cargo test -p aura-harness missing_sync_prerequisites_fail_as_convergence_contract_violations --quiet
cargo test -p aura-harness semantic_wait_helpers_do_not_use_raw_dom_or_text_fallbacks --quiet
cargo test -p aura-harness raw_text_fallbacks_are_explicitly_diagnostic_only --quiet
cargo test -p aura-harness registered_recoveries_cover_all_paths --quiet
cargo test -p aura-harness extracts_structured_command_metadata --quiet

echo "shared flow policy: clean"
