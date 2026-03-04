#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

run_pair() {
  local baseline="$1"
  local current="$2"
  cargo run -q -p aura-testkit --example protocol_compat -- "$baseline" "$current"
}

run_fixture_compatible() {
  local base="crates/aura-testkit/fixtures/protocol_compat/compatible_baseline.choreo"
  local curr="crates/aura-testkit/fixtures/protocol_compat/compatible_current.choreo"
  echo "checking known-compatible fixture..."
  run_pair "$base" "$curr"
}

run_fixture_breaking() {
  local base="crates/aura-testkit/fixtures/protocol_compat/breaking_baseline.choreo"
  local curr="crates/aura-testkit/fixtures/protocol_compat/breaking_current.choreo"
  echo "checking known-breaking fixture..."
  run_pair "$base" "$curr"
}

if [[ "${1:-}" == "--pair" ]]; then
  if [[ "$#" -ne 3 ]]; then
    echo "usage: scripts/check-protocol-compat.sh --pair <baseline.choreo> <current.choreo>"
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

changed_choreo="$(git diff --name-only "$BASE_REF"...HEAD -- '*.choreo')"
if [[ -z "$changed_choreo" ]]; then
  echo "no changed choreography files vs $BASE_REF; compatibility check skipped"
  exit 0
fi

status=0
for file in $changed_choreo; do
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
  if ! run_pair "$baseline_tmp" "$file"; then
    status=1
  fi
  rm -f "$baseline_tmp"
done

exit "$status"
