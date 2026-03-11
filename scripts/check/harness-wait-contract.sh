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
rg -q 'let mut waits = WaitCoordinator::new' crates/aura-harness/src/executor.rs \
  || fail "shared semantic execution must instantiate WaitCoordinator"
rg -Fq 'waits.modal(' crates/aura-harness/src/executor.rs \
  || fail "shared semantic execution must route modal waits through WaitCoordinator"
rg -Fq 'waits.runtime_event(' crates/aura-harness/src/executor.rs \
  || fail "shared semantic execution must route runtime-event waits through WaitCoordinator"
rg -Fq 'waits.semantic_state(' crates/aura-harness/src/executor.rs \
  || fail "shared semantic execution must route semantic waits through WaitCoordinator"
rg -q 'fn operation_state\(' crates/aura-harness/src/executor.rs \
  || fail "WaitCoordinator must define typed operation wait support"

echo "harness wait contract: clean"
