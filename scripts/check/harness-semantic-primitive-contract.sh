#!/usr/bin/env bash
# Verify canonical shared scenario model exposes typed intent actions.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-semantic-primitive-contract: $*" >&2
  exit 1
}

scenario_contract_files=(
  crates/aura-app/src/scenario_contract.rs
  crates/aura-app/src/scenario_contract/*.rs
)

rg -q 'ScenarioAction::Intent' "${scenario_contract_files[@]}" \
  || fail "canonical shared scenario model must expose typed intent actions"
rg -q 'validate_shared_intent_contract' "${scenario_contract_files[@]}" \
  || fail "shared scenario validation must enforce typed intent actions"

cargo test -p hxrts-aura-app shared_intent_contract_accepts_intents --quiet
cargo test -p hxrts-aura-app shared_intent_contract_rejects_ui_actions --quiet

echo "harness semantic primitive contract: clean"
