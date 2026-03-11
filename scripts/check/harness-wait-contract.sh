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

raw_wait_hits="$(rg --no-heading -n \
  'wait_for_modal\(|wait_for_runtime_event\(|wait_for_semantic_state\(|wait_for_operation_handle_state\(' \
  crates/aura-harness/src/executor.rs || true)"

filtered_hits="$(printf '%s\n' "$raw_wait_hits" | grep -v 'fn wait_for_' | grep -v 'self.tool_api' || true)"
if [ -n "$filtered_hits" ]; then
  echo "$filtered_hits" >&2
  fail "parity-critical waits must flow through WaitCoordinator and WaitContractRef"
fi

echo "harness wait contract: clean"
