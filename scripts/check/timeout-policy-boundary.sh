#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/timeout-policy-boundary.allowlist"

fail() {
  echo "timeout-policy-boundary: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

# Thin inventory check: parity-critical workflow/runtime/interface paths should
# use shared timeout-budget helpers instead of direct timeout/sleep primitives,
# except for a temporary set of approved low-level wrappers.

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
      -e 'tokio::time::timeout\(' \
      -e 'tokio::time::sleep\(' \
      -e 'std::thread::sleep\(' \
      -e 'thread::sleep\(' \
      crates/aura-app/src/workflows \
      crates/aura-agent/src/handlers/invitation \
      crates/aura-agent/src/runtime_bridge \
      crates/aura-agent/src/runtime/effects \
      crates/aura-terminal/src/tui \
      crates/aura-harness/src \
      -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "parity-critical code still uses direct timeout/sleep primitives outside sanctioned wrappers"
fi

echo "timeout policy boundary: clean (${legacy_exemptions} temporary exemptions)"
