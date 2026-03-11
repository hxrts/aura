#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-semantic-primitive-contract: $*" >&2
  exit 1
}

rg -q 'ScenarioAction::Intent' crates/aura-app/src/scenario_contract.rs \
  || fail "canonical shared scenario model must expose typed intent actions"
rg -q 'validate_shared_intent_contract' crates/aura-app/src/scenario_contract.rs \
  || fail "shared scenario validation must enforce typed intent actions"

cargo test -p aura-app shared_intent_contract_accepts_intents --quiet
cargo test -p aura-app shared_intent_contract_rejects_ui_actions --quiet

echo "harness semantic primitive contract: clean"
