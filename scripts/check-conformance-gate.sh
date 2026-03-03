#!/usr/bin/env bash
set -euo pipefail

legacy_workflow=".github/workflows/ci.yml"
workflow=".github/workflows/conform.yml"

fail() {
  echo "[conformance-gate] ERROR: $1" >&2
  exit 1
}

check_triggers() {
  local file="$1"
  if ! rg -q 'pull_request:' "$file" || ! rg -q 'branches:\s*\[main, develop\]' "$file"; then
    fail "Conformance gate must run on pull_request for protected branches. Ensure trigger includes 'pull_request' with '[main, develop]' in $file."
  fi
}

if [[ -f "$workflow" ]]; then
  if ! rg -q '^\s{2}conformance:' "$workflow"; then
    fail "Missing 'conformance' job in $workflow. Add job 'conformance' that runs 'nix develop --command just ci-conformance'."
  fi

  if ! rg -q 'just ci-conformance-policy' "$workflow"; then
    fail "Conformance workflow must execute 'just ci-conformance-policy'."
  fi

  if ! rg -q 'just ci-conformance' "$workflow"; then
    fail "Conformance workflow must execute 'just ci-conformance'."
  fi

  check_triggers "$workflow"

  if ! rg -q 'upload-artifact@v4' "$workflow" || ! rg -q 'artifacts/conformance' "$workflow"; then
    fail "Conformance workflow must upload conformance traces/diffs as artifacts. Add actions/upload-artifact@v4 step for artifacts/conformance."
  fi

  echo "[conformance-gate] OK: conformance gate wiring is present in $workflow"
  exit 0
fi

if [[ ! -f "$legacy_workflow" ]]; then
  fail "Missing $workflow (or legacy $legacy_workflow). Add a protected-branch conformance workflow that runs 'just ci-conformance'."
fi

if ! rg -q '^\s{2}conformance-gate:' "$legacy_workflow"; then
  fail "Missing 'conformance-gate' job in $legacy_workflow. Add job 'conformance-gate' that runs 'nix develop --command just ci-conformance'."
fi

if ! rg -q 'just ci-conformance' "$legacy_workflow"; then
  fail "Conformance gate job must execute 'just ci-conformance'."
fi

check_triggers "$legacy_workflow"

if ! rg -q 'upload-artifact@v4' "$legacy_workflow" || ! rg -q 'artifacts/conformance' "$legacy_workflow"; then
  fail "Conformance gate must upload conformance traces/diffs as artifacts. Add actions/upload-artifact@v4 step for artifacts/conformance."
fi

echo "[conformance-gate] OK: conformance gate wiring is present in $legacy_workflow"
