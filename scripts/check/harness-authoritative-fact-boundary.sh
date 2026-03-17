#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Temporary exemptions (owner: architecture, doc: work/ownership.md)
allowlist=()

fail() {
  echo "harness-authoritative-fact-boundary: $*" >&2
  exit 1
}

# Frontend-facing modules should not start constructing or taking ownership of
# authoritative semantic facts outside approved bridges/coordinators.
# Pattern matching in existing downstream observation modules is temporarily
# allowlisted until that mirror path is removed.

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  allowed=0
  for pattern in "${allowlist[@]}"; do
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      legacy_exemptions=$((legacy_exemptions + 1))
      break
    fi
  done

  if (( allowed == 0 )); then
    violations+=("$match")
  fi
done < <(
  {
    rg -n \
      -e 'AuthoritativeSemanticFact::(OperationStatus|PendingHomeInvitationReady|ContactLinkReady|ChannelMembershipReady|RecipientPeersResolved|PeerChannelReady|MessageDeliveryReady)' \
      crates/aura-terminal/src crates/aura-web/src crates/aura-harness/src -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "frontend-facing modules are handling authoritative semantic facts outside approved boundaries"
fi

echo "harness authoritative fact boundary: clean (${legacy_exemptions} temporary exemptions)"
