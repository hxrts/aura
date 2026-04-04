#!/usr/bin/env bash
# Verify runtime lifecycle bridge signatures use typed returns, not unit.
set -euo pipefail

cd "$(dirname "$0")/../.."

fail() {
  echo "runtime-typed-lifecycle-bridge: $1" >&2
  exit 1
}

check_absent() {
  local pattern="$1"
  shift
  if rg -n "$pattern" "$@" >/dev/null; then
    fail "forbidden unit-return lifecycle signature matched: $pattern"
  fi
}

check_present() {
  local pattern="$1"
  shift
  if ! rg -n "$pattern" "$@" >/dev/null; then
    fail "required typed lifecycle surface missing: $pattern"
  fi
}

app_bridge_files=(
  crates/aura-app/src/runtime_bridge.rs
  crates/aura-app/src/runtime_bridge/*.rs
)
agent_bridge="crates/aura-agent/src/runtime_bridge/mod.rs"
agent_rendezvous="crates/aura-agent/src/runtime_bridge/rendezvous.rs"
mock_bridge="crates/aura-testkit/src/mock_runtime_bridge.rs"

check_absent 'async fn process_ceremony_messages\(&self\) -> Result<\(\), IntentError>' \
  "${app_bridge_files[@]}" "$agent_bridge" "$mock_bridge"
check_absent 'async fn trigger_discovery\(&self\) -> Result<\(\), IntentError>' \
  "${app_bridge_files[@]}" "$agent_bridge" "$agent_rendezvous" "$mock_bridge"
check_absent 'async fn accept_invitation\([^)]*\) -> Result<\(\), IntentError>' \
  "${app_bridge_files[@]}" "$agent_bridge" "$mock_bridge"
check_absent 'async fn decline_invitation\([^)]*\) -> Result<\(\), IntentError>' \
  "${app_bridge_files[@]}" "$agent_bridge" "$mock_bridge"
check_absent 'async fn cancel_invitation\([^)]*\) -> Result<\(\), IntentError>' \
  "${app_bridge_files[@]}" "$agent_bridge" "$mock_bridge"

check_present 'enum CeremonyProcessingOutcome' "${app_bridge_files[@]}"
check_present 'enum DiscoveryTriggerOutcome' "${app_bridge_files[@]}"
check_present 'struct InvitationMutationOutcome' "${app_bridge_files[@]}"

echo "runtime-typed-lifecycle-bridge: clean"
