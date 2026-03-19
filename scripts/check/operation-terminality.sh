#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "operation-terminality: $*" >&2
  exit 1
}

# Thin escape-hatch invariant:
# Phase 2 moves primary terminality enforcement into macro-generated lifecycle
# APIs and semantic-owner declarations. This wrapper only protects interface
# layers from reintroducing authoritative terminal publication outside the
# sanctioned app workflow boundary.

violations=()

while IFS= read -r match; do
  [[ -z "$match" ]] && continue
  violations+=("$match")
done < <(
  rg -n 'publish_authoritative_operation_(phase|failure)(_with_instance)?\(' \
    crates/aura-terminal \
    crates/aura-web \
    -g '*.rs' \
    | sort
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "interface-layer code is bypassing macro-enforced terminal publication boundaries"
fi

echo "operation terminality: clean"
