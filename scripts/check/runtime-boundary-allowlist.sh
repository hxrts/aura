#!/usr/bin/env bash
# Unified allowlist check for runtime boundary policies.
#
# Modes:
#   instrumentation  — runtime event names must come from runtime/instrumentation.rs
#   concurrency      — concurrency envelope selection must stay in vm_hardening / vm_host_bridge / choreo_engine
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mode="${1:-}"
if [[ -z "$mode" ]]; then
  echo "usage: runtime-boundary-allowlist.sh <instrumentation|concurrency>" >&2
  exit 1
fi

case "$mode" in
  instrumentation)
    label="runtime instrumentation schema"
    approved_sites=(
      '^crates/aura-agent/src/task_registry\.rs:'
      '^crates/aura-agent/src/runtime/services/ceremony_tracker\.rs:'
      '^crates/aura-agent/src/runtime/services/rendezvous_manager\.rs:'
      '^crates/aura-agent/src/runtime/services/maintenance_service\.rs:'
      '^crates/aura-agent/src/runtime/services/sync_manager\.rs:'
      '^crates/aura-agent/src/runtime/system\.rs:'
    )
    skip_pattern='^crates/aura-agent/src/runtime/instrumentation\.rs:'
    matches=$(
      rg -n 'event\s*=\s*"runtime\.' crates/aura-agent/src/runtime crates/aura-agent/src/task_registry.rs -g '*.rs' \
        | rg -v ':\s*//!|:\s*//|:\s*/\*'
    ) || true
    fail_msg="runtime event names must come from runtime/instrumentation.rs or be explicitly allowlisted"
    ;;
  concurrency)
    label="async concurrency envelope"
    approved_sites=(
      '^crates/aura-agent/src/runtime/contracts\.rs:.*canonical_fallback_policy\('
    )
    skip_pattern='^crates/aura-agent/src/runtime/(vm_hardening|vm_host_bridge|choreo_engine)\.rs:'
    matches=$(
      rg -n \
        -e 'AuraVmRuntimeMode::ThreadedReplayDeterministic' \
        -e 'AuraVmRuntimeMode::ThreadedEnvelopeBounded' \
        -e 'AuraVmRuntimeSelector::for_policy\(' \
        -e 'new_with_contracts_and_selector\(' \
        -e 'canonical_fallback_policy\(' \
        crates/aura-agent/src -g '*.rs' \
        | rg -v ':\s*//!|:\s*//|:\s*/\*'
    ) || true
    fail_msg="non-admitted concurrency path bypasses vm_hardening.rs / vm_host_bridge.rs / choreo_engine.rs"
    ;;
  *)
    echo "unknown mode: $mode (expected instrumentation or concurrency)" >&2
    exit 1
    ;;
esac

violations=()
approved_hits=0

while IFS= read -r match; do
  [[ -z "$match" ]] && continue

  if [[ "$match" =~ $skip_pattern ]]; then
    continue
  fi

  allowed=0
  for pattern in "${approved_sites[@]}"; do
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      approved_hits=$((approved_hits + 1))
      break
    fi
  done

  if (( allowed == 0 )); then
    violations+=("$match")
  fi
done <<< "$matches"

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  echo "$label: $fail_msg" >&2
  exit 1
fi

echo "$label: clean (${approved_hits} approved boundary hits)"
