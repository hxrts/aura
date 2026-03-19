#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Approved boundary modules and compile-fail ownership tests.
approved_sites=(
  '^crates/aura-app/src/ui_contract\.rs:'
  '^crates/aura-app/src/scenario_contract\.rs:'
  '^crates/aura-app/src/workflows/harness_determinism\.rs:'
  '^crates/aura-app/tests/ui/ui_operation_handle_private_fields\.rs:'
  '^crates/aura-app/tests/ui/harness_ui_operation_handle_private_fields\.rs:'
  '^crates/aura-harness/src/backend/local_pty\.rs:'
  '^crates/aura-harness/src/backend/mod\.rs:'
  '^crates/aura-harness/src/executor\.rs:'
  '^crates/aura-terminal/src/tui/harness_state\.rs:'
  '^crates/aura-terminal/src/tui/harness_state/mod\.rs:'
  '^crates/aura-terminal/src/tui/semantic_lifecycle\.rs:'
  '^crates/aura-terminal/src/tui/screens/app/shell\.rs:'
)

fail() {
  echo "harness-move-ownership-boundary: $*" >&2
  exit 1
}

# Shared semantic move ownership is currently expressed through:
# - sanctioned UiOperationHandle / HarnessUiOperationHandle construction
# - handle recording
# - HarnessUiCommandReceipt acceptance / rejection
# - sanctioned instance-id capture helpers
#
# New ambient ownership mutation sites should not appear outside the approved
# boundary modules while we migrate toward stronger owner-token / handoff
# objects.

violations=()
approved_hits=0

while IFS= read -r match; do
  allowed=0
  for pattern in "${approved_sites[@]}"; do
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      approved_hits=$((approved_hits + 1))
      break
    fi
  done

  if (( allowed == 0 )); then
    violations+=("$match")
  fi
done < <(
  {
    rg -n \
      -e 'UiOperationHandle::new\(' \
      -e 'HarnessUiOperationHandle::new\(' \
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

echo "harness move ownership boundary: clean (${approved_hits} approved boundary hits)"
