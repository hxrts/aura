#!/usr/bin/env bash
set -euo pipefail

workflow=".github/workflows/ci.yml"

fail() {
  echo "[conformance-gate] ERROR: $1" >&2
  exit 1
}

if [[ ! -f "$workflow" ]]; then
  fail "Missing $workflow. Add a protected-branch CI workflow with a conformance gate job that runs 'just ci-conformance'."
fi

if ! rg -q '^\s{2}conformance-gate:' "$workflow"; then
  fail "Missing 'conformance-gate' job in $workflow. Add job 'conformance-gate' that runs 'nix develop --command just ci-conformance'."
fi

if ! rg -q 'just ci-conformance' "$workflow"; then
  fail "Conformance gate job must execute 'just ci-conformance'."
fi

if ! rg -q 'pull_request:' "$workflow" || ! rg -q 'branches:\s*\[main, develop\]' "$workflow"; then
  fail "Conformance gate must run on pull_request for protected branches. Ensure CI trigger includes 'pull_request' with '[main, develop]'."
fi

if ! rg -q 'upload-artifact@v4' "$workflow" || ! rg -q 'artifacts/conformance' "$workflow"; then
  fail "Conformance gate must upload conformance traces/diffs as artifacts. Add actions/upload-artifact@v4 step for artifacts/conformance."
fi

echo "[conformance-gate] OK: conformance gate wiring is present in $workflow"
