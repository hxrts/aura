#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Temporary exemptions (owner: architecture, doc: work/ownership.md)
allowlist=()

fail() {
  echo "runtime-service-lifecycle: $*" >&2
  exit 1
}

violations=()
legacy_exemptions=0

while IFS= read -r match; do
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
  {
    rg -n \
      -e 'synchronous approximation' \
      -e 'Placeholder' \
      -e 'explicit start\(time_effects\)' \
      -e 'call start_cleanup_task' \
      -e 'call start_maintenance_task' \
      crates/aura-agent/src/runtime/services -g '*.rs'
    rg -n \
      -e '\.start\(time_effects\)' \
      -e 'start_cleanup_task\(' \
      -e 'start_maintenance_task\(' \
      crates/aura-agent/src/runtime/system.rs \
      crates/aura-agent/src/runtime/builder.rs
  } | sort -u
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "placeholder lifecycle/health or lifecycle bypass detected"
fi

echo "runtime service lifecycle: clean (${legacy_exemptions} temporary exemptions)"
