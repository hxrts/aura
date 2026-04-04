#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness focus selection contract: $*" >&2
  exit 1
}

scenario_contract_files=(
  crates/aura-app/src/scenario_contract.rs
  crates/aura-app/src/scenario_contract/*.rs
)

rg -q 'pub enum FocusSemantics' "${scenario_contract_files[@]}" \
  || fail "missing focus semantics contract"
rg -q 'pub enum SelectionSemantics' "${scenario_contract_files[@]}" \
  || fail "missing selection semantics contract"
rg -q 'pub struct SharedActionContract' "${scenario_contract_files[@]}" \
  || fail "missing shared action contract"

cargo test -p aura-app every_intent_kind_declares_focus_and_selection_semantics --quiet
cargo test -p aura-app ui_snapshot_parity_detects_focus_semantic_drift --quiet

echo "harness focus selection contract: clean"
