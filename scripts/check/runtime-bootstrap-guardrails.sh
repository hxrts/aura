#!/usr/bin/env bash
# Check for forbidden runtime bootstrap and error-boundary patterns.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

violations=0

check_pattern() {
  local description="$1"
  local pattern="$2"
  shift 2
  local output

  output="$(rg -n "$pattern" "$@" || true)"
  if [[ -n "$output" ]]; then
    echo "✖ $description"
    echo "$output"
    echo
    violations=$((violations + 1))
  fi
}

check_pattern \
  "preset builder authority fallback detected" \
  'self\.authority_id\.(unwrap_or|unwrap_or_else)\(' \
  crates/aura-agent/src/builder

check_pattern \
  "preset builder creates authority directly instead of requiring explicit bootstrap identity" \
  'AuthorityId::new_from_entropy\(|new_authority_id\(' \
  crates/aura-agent/src/builder

check_pattern \
  "terminal main still hard-codes synthetic startup authority/context" \
  'ids::authority_id\("cli:main-authority"\)|ids::context_id\("cli:main-context"\)' \
  crates/aura-terminal/src/main.rs

if [[ "$violations" -ne 0 ]]; then
  echo "bootstrap-guardrails: found $violations bootstrap guardrail violation(s)"
  exit 1
fi

echo "bootstrap-guardrails: clean"
