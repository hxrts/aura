#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/timeout-backoff-discipline.allowlist"

fail() {
  echo "timeout-backoff-discipline: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

# Thin inventory check: parity-critical workflow/runtime/interface paths should
# not hand-roll retry/backoff loops when they should use the shared retry
# budget/backoff model.

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  [[ -z "$match" ]] && continue

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
  {
    rg -n \
      -e 'backoff\s*=\s*\(backoff \* 2\)\.min' \
      -e 'sleep\(backoff\)' \
      -e 'retry_interval_ms' \
      -e 'for attempt in ' \
      -e 'attempts \+=' \
      crates/aura-app/src/workflows \
      crates/aura-agent/src/handlers/invitation \
      crates/aura-agent/src/runtime_bridge \
      crates/aura-terminal/src/tui \
      crates/aura-harness/src \
      -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "parity-critical code still contains duplicated retry/backoff logic outside the shared timeout model"
fi

echo "timeout backoff discipline: clean (${legacy_exemptions} temporary exemptions)"
