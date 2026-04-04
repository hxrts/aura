#!/usr/bin/env bash
# Reject forbidden optional-authority device enrollment patterns.
set -euo pipefail

cd "$(dirname "$0")/../.."

forbidden_patterns=(
  'invitee_authority_id: Option<AuthorityId>'
  '_invitee_authority_id: Option<AuthorityId>'
  'start_device_enrollment\([^)]*, None\)'
  'invitee_authority_id: None'
  'unwrap_or\(authority_id\)'
)

targets=(
  'crates/aura-app'
  'crates/aura-agent'
  'crates/aura-terminal'
  'crates/aura-testkit'
)

failed=0

for pattern in "${forbidden_patterns[@]}"; do
  if rg -n "$pattern" "${targets[@]}" >/tmp/device-enrollment-authority-contract.out 2>/dev/null; then
    echo "device-enrollment-authority-contract: forbidden pattern matched: $pattern"
    cat /tmp/device-enrollment-authority-contract.out
    failed=1
  fi
done

if [[ $failed -ne 0 ]]; then
  exit 1
fi

echo "device-enrollment-authority-contract: clean"
