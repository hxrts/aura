#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/harness-semantic-lifecycle-ownership.allowlist"
approved_bridge_pattern='^crates/aura-terminal/src/tui/semantic_lifecycle\.rs:'

fail() {
  echo "harness-semantic-lifecycle-ownership: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

# Authoritative semantic lifecycle ownership must stay in:
# - aura-app workflow/coordinator modules
# - the dedicated terminal semantic lifecycle bridge
#
# Frontend shell/callback/subscription modules may observe lifecycle, but they
# must not become parallel authors of terminal semantic truth.

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  allowed=0
  if [[ "$match" =~ $approved_bridge_pattern ]]; then
    continue
  fi
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
      -e 'publish_authoritative_operation_phase\(' \
      -e 'publish_authoritative_operation_failure\(' \
      -e 'set_operation_state\(OperationId::(create_home|invitation_create|invitation_accept|join_channel|send_message)\(' \
      crates/aura-terminal/src crates/aura-web/src crates/aura-harness/src -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "frontend/harness modules author shared semantic lifecycle outside approved coordinators"
fi

echo "harness semantic lifecycle ownership: clean (${legacy_exemptions} temporary exemptions)"
