#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo test -p aura-app shared_flow_support_contract_is_consistent --quiet
cargo test -p aura-app shared_flow_scenario_coverage_points_to_existing_scenarios --quiet
cargo test -p aura-app shared_screen_modal_and_list_support_is_unique_and_addressable --quiet
cargo test -p aura-app shared_screen_module_map_uses_canonical_screen_names --quiet
cargo test -p aura-app parity_module_map_points_to_existing_frontend_symbols --quiet
cargo test -p aura-app parity_exception_metadata_is_complete_and_documented --quiet

echo "ui parity contract: clean"
