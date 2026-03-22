#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-wait-contract: $*" >&2
  exit 1
}

rg -q 'enum WaitContractRef' crates/aura-harness/src/executor.rs \
  || fail "missing typed wait contract reference"
rg -q 'fn ensure_wait_contract_declared' crates/aura-harness/src/executor.rs \
  || fail "shared semantic waits must validate declared barrier contracts"
rg -Fq 'waits.modal(' crates/aura-harness/src/executor.rs \
  || rg -q 'fn wait_for_modal\(' crates/aura-harness/src/executor.rs \
  || fail "shared semantic execution must route modal waits through the typed wait contract"
rg -Fq 'BarrierDeclaration::RuntimeEvent' crates/aura-harness/src/executor.rs \
  || rg -q 'fn wait_for_runtime_event_snapshot\(' crates/aura-harness/src/executor.rs \
  || fail "shared semantic execution must route runtime-event waits through the typed wait contract"
rg -Fq 'waits.semantic_state(' crates/aura-harness/src/executor.rs \
  || rg -q 'fn wait_for_semantic_state\(' crates/aura-harness/src/executor.rs \
  || fail "shared semantic execution must route semantic waits through the typed wait contract"
rg -q 'WaitContractRef::OperationState' crates/aura-harness/src/executor.rs \
  || fail "typed wait contracts must include operation wait support"
rg -Fq 'snapshot.operation_state(' crates/aura-harness/src/executor.rs \
  || fail "typed wait contracts must read operation state through the shared snapshot surface"
cargo test -p aura-harness shared_intent_waits_bind_only_to_declared_barriers --quiet \
  || fail "shared semantic waits must bind only to declared barriers"

echo "harness wait contract: clean"
