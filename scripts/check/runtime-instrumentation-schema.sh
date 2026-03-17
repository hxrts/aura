#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Temporary exemptions (owner: architecture, doc: work/ownership.md)
allowlist=(
  '^crates/aura-agent/src/task_registry\.rs:'
  '^crates/aura-agent/src/runtime/services/ceremony_tracker\.rs:'
  '^crates/aura-agent/src/runtime/services/rendezvous_manager\.rs:'
  '^crates/aura-agent/src/runtime/services/maintenance_service\.rs:'
  '^crates/aura-agent/src/runtime/services/sync_manager\.rs:'
  '^crates/aura-agent/src/runtime/system\.rs:'
)

fail() {
  echo "runtime-instrumentation-schema: $*" >&2
  exit 1
}

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  if [[ "$match" =~ ^crates/aura-agent/src/runtime/instrumentation\.rs: ]]; then
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
  rg -n 'event\s*=\s*"runtime\.' crates/aura-agent/src/runtime crates/aura-agent/src/task_registry.rs -g '*.rs' \
    | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "runtime event names must come from runtime/instrumentation.rs or be explicitly allowlisted"
fi

echo "runtime instrumentation schema: clean (${legacy_exemptions} temporary exemptions)"
