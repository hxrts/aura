#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/async-task-ownership.allowlist"

fail() {
  echo "async-task-ownership: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  if [[ "$match" =~ ^crates/aura-agent/src/task_registry\.rs: ]]; then
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
  rg -n 'tokio::spawn|spawn_local' crates/aura-agent/src -g '*.rs' \
    | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "raw task spawning bypasses TaskSupervisor/TaskGroup"
fi

echo "async task ownership: clean (${legacy_exemptions} temporary exemptions)"
