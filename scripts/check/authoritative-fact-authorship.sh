#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Temporary exemptions (owner: architecture, doc: work/ownership.md)
allowlist=()

approved_owner_pattern='^crates/aura-app/src/workflows/.*\.rs:'
approved_bridge_pattern='^crates/aura-terminal/src/tui/semantic_lifecycle\.rs:'

fail() {
  echo "authoritative-fact-authorship: $*" >&2
  exit 1
}

# Authoritative lifecycle/readiness publication must stay in the shared
# aura-app workflow/coordinator surface or the dedicated terminal semantic
# lifecycle bridge. Other layers may observe or submit, but may not become
# parallel authors of semantic truth.

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  [[ -z "$match" ]] && continue

  if [[ "$match" =~ $approved_owner_pattern ]]; then
    continue
  fi
  if [[ "$match" =~ $approved_bridge_pattern ]]; then
    continue
  fi

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
      -e 'publish_authoritative_semantic_fact\(' \
      -e 'publish_authoritative_operation_phase(_with_instance)?\(' \
      -e 'publish_authoritative_operation_failure(_with_instance)?\(' \
      -e 'publish_authoritative_operation_cancellation\(' \
      -e 'replace_authoritative_semantic_facts_of_kind\(' \
      -e 'set_operation_state\(OperationId::' \
      crates -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "authoritative lifecycle/readiness publication exists outside approved coordinator modules"
fi

echo "authoritative fact authorship: clean (${legacy_exemptions} temporary exemptions)"
