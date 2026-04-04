#!/usr/bin/env bash
# Run protocol compatibility pair tests across session-type baselines.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

run_pair() {
  local baseline="$1"
  local current="$2"
  AURA_PROTOCOL_COMPAT_BASELINE="$baseline" \
    AURA_PROTOCOL_COMPAT_CURRENT="$current" \
    cargo test -q -p aura-testkit protocol_compat_pair_from_env -- --ignored --exact --nocapture
}

has_dynamic_roles() {
  local file="$1"
  rg -q '\[\*\]' "$file"
}

has_named_role_choice() {
  local file="$1"
  rg -q '^\s*choice at [A-Za-z_][A-Za-z0-9_]*' "$file"
}

run_fixture_compatible() {
  local base="crates/aura-testkit/fixtures/protocol_compat/compatible_baseline.tell"
  local curr="crates/aura-testkit/fixtures/protocol_compat/compatible_current.tell"
  echo "checking known-compatible fixture..."
  run_pair "$base" "$curr"
}

run_fixture_breaking() {
  local base="crates/aura-testkit/fixtures/protocol_compat/breaking_baseline.tell"
  local curr="crates/aura-testkit/fixtures/protocol_compat/breaking_current.tell"
  echo "checking known-breaking fixture..."
  run_pair "$base" "$curr"
}

if [[ "${1:-}" == "--pair" ]]; then
  if [[ "$#" -ne 3 ]]; then
    echo "usage: scripts/check/protocol-compat.sh --pair <baseline.tell> <current.tell>"
    exit 2
  fi
  run_pair "$2" "$3"
  exit 0
fi

if [[ "${1:-}" == "--fixture-compatible" ]]; then
  run_fixture_compatible
  exit 0
fi

if [[ "${1:-}" == "--fixture-breaking" ]]; then
  run_fixture_breaking
  exit 0
fi

if [[ "${1:-}" == "--self-test" ]]; then
  run_fixture_compatible
  if run_fixture_breaking; then
    echo "expected incompatibility but known-breaking fixture passed"
    exit 1
  fi
  exit 0
fi

BASE_REF="${BASE_REF:-origin/main}"
if ! git rev-parse --verify "$BASE_REF" >/dev/null 2>&1; then
  BASE_REF="${BASE_REF_FALLBACK:-HEAD~1}"
fi

if ! git rev-parse --verify "$BASE_REF" >/dev/null 2>&1; then
  echo "could not resolve baseline git ref (tried origin/main and HEAD~1)"
  exit 2
fi

changed_tell="$(git diff --name-only "$BASE_REF"...HEAD -- '*.tell')"
if [[ -z "$changed_tell" ]]; then
  echo "no changed choreography files vs $BASE_REF; compatibility check skipped"
  exit 0
fi

status=0
for file in $changed_tell; do
  if [[ ! -f "$file" ]]; then
    echo "skipping deleted choreography: $file"
    continue
  fi
  if ! git cat-file -e "${BASE_REF}:${file}" 2>/dev/null; then
    echo "skipping new choreography without baseline: $file"
    continue
  fi

  echo "checking protocol compatibility: $file"
  baseline_tmp="$(mktemp)"
  git show "${BASE_REF}:${file}" > "$baseline_tmp"
  if has_dynamic_roles "$baseline_tmp" || has_dynamic_roles "$file"; then
    echo "skipping dynamic-role choreography without static projection support: $file"
    rm -f "$baseline_tmp"
    continue
  fi
  if has_named_role_choice "$baseline_tmp" || has_named_role_choice "$file"; then
    echo "skipping named-role choice choreography without async-subtype parser support: $file"
    rm -f "$baseline_tmp"
    continue
  fi
  if ! run_pair "$baseline_tmp" "$file"; then
    status=1
  fi
  rm -f "$baseline_tmp"
done

exit "$status"
