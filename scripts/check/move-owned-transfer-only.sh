#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist=()

fail() {
  echo "move-owned-transfer-only: $*" >&2
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
  rg -n \
    -e 'self\.owner\s*=' \
    -e 'operation\.instance_id\s*=' \
    -e 'operations\[[0-9]+\]\.instance_id\s*=' \
    crates/*/src -g '*.rs' \
    | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "direct move-owned owner or instance rewrites bypass sanctioned transfer boundaries"
fi

echo "move-owned transfer only: clean (${legacy_exemptions} temporary exemptions)"
