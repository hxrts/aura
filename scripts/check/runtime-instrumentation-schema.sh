#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/runtime-instrumentation-schema.allowlist"

fail() {
  echo "runtime-instrumentation-schema: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  if [[ "$match" =~ ^crates/aura-agent/src/runtime/instrumentation\.rs: ]]; then
    continue
  fi

  allowed=0
  while IFS= read -r pattern; do
    [[ -z "$pattern" || "$pattern" =~ ^# ]] && continue
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      legacy_exemptions=$((legacy_exemptions + 1))
      break
    fi
  done < "$allowlist_file"

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
