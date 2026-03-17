#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Temporary exemptions (owner: architecture, doc: work/ownership.md)
allowlist=(
  '^crates/aura-agent/src/handlers/recovery_service\.rs:.*tokio::spawn'
  '^crates/aura-agent/src/reactive/frp\.rs:.*tokio::spawn'
  '^crates/aura-agent/src/reactive/pipeline\.rs:.*tokio::spawn'
  '^crates/aura-agent/src/reactive/pipeline\.rs:.*spawn_local'
  '^crates/aura-agent/src/reactive/scheduler\.rs:.*tokio::spawn'
  '^crates/aura-agent/src/runtime/effects/choreography\.rs:.*tokio::spawn'
  '^crates/aura-agent/src/runtime/effects/network\.rs:.*tokio::spawn'
  '^crates/aura-agent/src/runtime/effects/network\.rs:.*spawn_local'
  '^crates/aura-agent/src/runtime/effects/transport\.rs:.*spawn_local'
  '^crates/aura-agent/src/runtime/services/lan_discovery\.rs:.*tokio::spawn'
  '^crates/aura-agent/src/runtime/services/rendezvous_manager\.rs:.*tokio::spawn'
  '^crates/aura-agent/src/runtime/services/rendezvous_manager\.rs:.*spawn_local'
  '^crates/aura-agent/src/runtime/system\.rs:.*tokio::spawn'
  '^crates/aura-agent/src/runtime_bridge/mod\.rs:.*spawn_local'
)

fail() {
  echo "async-task-ownership: $*" >&2
  exit 1
}

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  if [[ "$match" =~ ^crates/aura-agent/src/task_registry\.rs: ]]; then
    continue
  fi

  allowed=0
  for pattern in "${allowlist[@]}"; do
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      legacy_exemptions=$((legacy_exemptions + 1))
      break
    fi
  done

  if (( allowed == 0 )); then
    violations+=("$match")
  fi
done < <(
  rg -n 'tokio::spawn|spawn_local' crates/aura-agent/src -g '*.rs' \
    | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "raw task spawning bypasses TaskSupervisor/TaskGroup"
fi

echo "async task ownership: clean (${legacy_exemptions} temporary exemptions)"
