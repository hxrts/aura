#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-action-preconditions: $*" >&2
  exit 1
}

scenario_contract="crates/aura-app/src/scenario_contract.rs"
executor="crates/aura-harness/src/executor.rs"

rg -q 'ActionPrecondition::Quiescence' "$scenario_contract" \
  || fail "shared action contracts must declare quiescence preconditions"
rg -q 'fn enforce_action_preconditions' "$executor" \
  || fail "executor is missing typed action precondition enforcement"
rg -Fq 'enforce_action_preconditions(step, tool_api, context, &intent)' "$executor" \
  || fail "shared action execution does not enforce preconditions before issue"
rg -q 'fn ensure_post_operation_convergence_satisfied' "$executor" \
  || fail "executor is missing typed post-operation convergence enforcement"
rg -Fq 'ensure_post_operation_convergence_satisfied(step, context, &instance_id)' "$executor" \
  || fail "shared action execution does not enforce post-operation convergence before the next intent"

cargo test -p aura-harness action_preconditions_fail_diagnostically_before_issue --quiet
cargo test -p aura-harness missing_sync_prerequisites_fail_as_convergence_contract_violations --quiet

echo "harness action preconditions: clean"
