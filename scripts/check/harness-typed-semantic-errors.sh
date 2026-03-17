#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Temporary exemptions (owner: architecture, doc: work/ownership.md)
allowlist=(
)

fail() {
  echo "harness-typed-semantic-errors: $*" >&2
  exit 1
}

# Parity-critical shared semantic paths should not rely on stringly frontend
# error construction as their primary contract. Those paths should map into
# typed semantic failure/status surfaces first, then format for display only at
# the edge.

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
      -e 'OpError::Failed\(format!' \
      -e 'TerminalError::Operation\(format!' \
      -e 'AuraError::agent\(format!' \
      crates/aura-app/src/workflows \
      crates/aura-terminal/src/tui/effects/operational \
      crates/aura-terminal/src/tui/context \
      crates/aura-web/src \
      crates/aura-harness/src \
      -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "parity-critical shared semantic paths still rely on stringly error construction"
fi

echo "harness typed semantic errors: clean (${legacy_exemptions} temporary exemptions)"
