#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

driver="crates/aura-harness/playwright-driver/src/playwright_driver.ts"

fail() {
  echo "browser semantic restart boundary: $*" >&2
  exit 1
}

[[ -f "$driver" ]] || fail "missing driver source: $driver"

if rg -n "pending_semantic_payload|pending_runtime_stage_payload" "$driver" >/dev/null; then
  fail "legacy restart seed payload plumbing is still present in the Playwright driver"
fi

submit_body="$(
  awk '
    /async function submitSemanticCommand\(params\)/ { in_block = 1 }
    in_block { print }
    /async function getAuthorityId\(params\)/ { exit }
  ' "$driver"
)"

runtime_body="$(
  awk '
    /async function stageRuntimeIdentity\(params\)/ { in_block = 1 }
    in_block { print }
    /async function domSnapshot\(params\)/ { exit }
  ' "$driver"
)"

[[ -n "$submit_body" ]] || fail "could not locate submitSemanticCommand in Playwright driver"
[[ -n "$runtime_body" ]] || fail "could not locate stageRuntimeIdentity in Playwright driver"

if grep -Fq "restartPageSession(" <<<"$submit_body"; then
  fail "submitSemanticCommand must fail closed instead of replaying through restartPageSession"
fi
if ! grep -Fq "submit_semantic_command_enqueue_failed_closed" <<<"$submit_body"; then
  fail "submitSemanticCommand no longer exposes an explicit fail-closed semantic enqueue path"
fi
if grep -Fq "restartPageSession(" <<<"$runtime_body"; then
  fail "stageRuntimeIdentity must fail closed instead of replaying through restartPageSession"
fi
if ! grep -Fq "stage_runtime_identity_enqueue_failed_closed" <<<"$runtime_body"; then
  fail "stageRuntimeIdentity no longer exposes an explicit fail-closed runtime-stage enqueue path"
fi

echo "browser semantic restart boundary: clean"
