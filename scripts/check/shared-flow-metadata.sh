#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-shared-flow-metadata: $*" >&2
  exit 1
}

scenario_contract_files=(
  crates/aura-app/src/scenario_contract.rs
  crates/aura-app/src/scenario_contract/*.rs
)

cargo test -p aura-app every_intent_kind_has_a_matching_contract --quiet \
  || fail "shared intent metadata contract is incomplete"
cargo test -p aura-app every_intent_kind_declares_barrier_metadata --quiet \
  || fail "shared intent barrier metadata contract is incomplete"
cargo test -p aura-app declared_post_operation_convergence_contracts_are_explicit --quiet \
  || fail "shared intent convergence metadata contract is incomplete"

rg -q 'pub struct SharedActionContract' "${scenario_contract_files[@]}" \
  || fail "missing SharedActionContract schema"
rg -q 'pub enum ActionPrecondition' "${scenario_contract_files[@]}" \
  || fail "missing ActionPrecondition schema"
rg -q 'pub struct SharedActionBarrierMetadata' "${scenario_contract_files[@]}" \
  || fail "missing SharedActionBarrierMetadata schema"
rg -q 'pub enum BarrierDeclaration' "${scenario_contract_files[@]}" \
  || fail "missing BarrierDeclaration schema"
rg -q 'pub struct PostOperationConvergenceContract' "${scenario_contract_files[@]}" \
  || fail "missing PostOperationConvergenceContract schema"

echo "harness shared-flow metadata: clean"
