#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-authoritative-fact-boundary: $*" >&2
  exit 1
}

# Frontend-facing modules should not start constructing or taking ownership of
# authoritative semantic facts outside approved bridges/coordinators.
violations="$(
  {
    rg -n \
      -e 'AuthoritativeSemanticFact::(OperationStatus|PendingHomeInvitationReady|ContactLinkReady|ChannelMembershipReady|RecipientPeersResolved|PeerChannelReady|MessageDeliveryReady)' \
      crates/aura-terminal/src crates/aura-web/src crates/aura-harness/src -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*' || true
)"

if [[ -n "$violations" ]]; then
  printf '%s\n' "$violations" >&2
  fail "frontend-facing modules are handling authoritative semantic facts outside approved boundaries"
fi

echo "harness authoritative fact boundary: clean (0 temporary exemptions)"
