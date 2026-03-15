#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/harness-readiness-ownership.allowlist"

fail() {
  echo "harness-readiness-ownership: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

# Authoritative readiness authorship must stay in approved workflow/runtime
# coordinators. Frontend and harness modules may observe readiness facts, but
# they must not publish or refresh them directly.

violations=()
legacy_exemptions=0

while IFS= read -r match; do
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
      -e 'refresh_authoritative_(invitation|contact_link|channel_membership|recipient_resolution|delivery)_readiness' \
      -e 'publish_authoritative_semantic_fact\(' \
      -e 'replace_authoritative_semantic_facts_of_kind\(' \
      crates/aura-terminal/src crates/aura-web/src crates/aura-harness/src -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "frontend/harness modules author or refresh authoritative readiness outside approved coordinators"
fi

echo "harness readiness ownership: clean (${legacy_exemptions} temporary exemptions)"
