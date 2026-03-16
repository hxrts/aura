#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "operation-terminality: $*" >&2
  exit 1
}

# Thin module-level invariant:
# canonical semantic workflow modules that publish authoritative operation
# phases must also define terminal failure publication and terminal
# success/cancellation publication surfaces.

violations=()

while IFS= read -r file; do
  [[ -z "$file" ]] && continue

  if ! rg -q 'publish_authoritative_operation_failure|SemanticOperationStatus::failed' "$file"; then
    violations+=("$file: missing terminal failure publication")
  fi

  if ! rg -q 'SemanticOperationPhase::Succeeded|SemanticOperationStatus::cancelled|publish_authoritative_operation_cancellation' "$file"; then
    violations+=("$file: missing terminal success/cancellation publication")
  fi
done < <(
  rg -l 'publish_authoritative_operation_phase(_with_instance)?\(' \
    crates/aura-app/src/workflows \
    -g '*.rs' \
    -g '!semantic_facts.rs' \
    | sort
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "semantic workflow modules are missing explicit terminal publication surfaces"
fi

echo "operation terminality: clean"
