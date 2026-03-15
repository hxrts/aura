#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/harness-move-ownership-boundary.allowlist"

fail() {
  echo "harness-move-ownership-boundary: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

# Shared semantic move ownership is currently expressed through:
# - UiOperationHandle fabrication and recording
# - HarnessUiCommandReceipt acceptance / rejection
# - sanctioned instance-id capture helpers
#
# New ambient ownership mutation sites should not appear outside the approved
# boundary modules while we migrate toward stronger owner-token / handoff
# objects.

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
      -e 'UiOperationHandle \{' \
      -e 'record_submission_handle\(' \
      -e 'HarnessUiCommandReceipt::Accepted' \
      -e 'instance_id\s*=\s*Some\(' \
      crates/aura-app crates/aura-terminal crates/aura-web crates/aura-harness -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "shared semantic move ownership escapes approved handle / receipt boundary modules"
fi

echo "harness move ownership boundary: clean (${legacy_exemptions} temporary exemptions)"
