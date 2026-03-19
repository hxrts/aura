#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Approved boundary modules for runtime concurrency envelope selection.
approved_sites=(
  '^crates/aura-agent/src/runtime/contracts\.rs:.*canonical_fallback_policy\('
)

fail() {
  echo "async-concurrency-envelope: $*" >&2
  exit 1
}

violations=()
approved_hits=0

while IFS= read -r match; do
  if [[ "$match" =~ ^crates/aura-agent/src/runtime/(vm_hardening|vm_host_bridge|choreo_engine)\.rs: ]]; then
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
done < <(
  rg -n \
    -e 'AuraVmRuntimeMode::ThreadedReplayDeterministic' \
    -e 'AuraVmRuntimeMode::ThreadedEnvelopeBounded' \
    -e 'AuraVmRuntimeSelector::for_policy\(' \
    -e 'new_with_contracts_and_selector\(' \
    -e 'canonical_fallback_policy\(' \
    crates/aura-agent/src -g '*.rs' \
    | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "non-admitted concurrency path bypasses vm_hardening.rs / vm_host_bridge.rs / choreo_engine.rs"
fi

echo "async concurrency envelope: clean (${approved_hits} approved boundary hits)"
