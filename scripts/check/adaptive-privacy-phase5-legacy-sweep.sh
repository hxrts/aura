#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "adaptive-privacy-phase5-legacy-sweep: $1" >&2
  exit 1
}

bash scripts/check/adaptive-privacy-runtime-locality.sh
bash scripts/check/transparent-onion-quarantine.sh

legacy_selection_hits="$(
  rg -n 'TransportSelector|CandidateKind|ConnectionCandidate|on_candidates_changed\(|select_establish_path(_with_probing)?\(' \
    crates/aura-rendezvous \
    crates/aura-protocol \
    crates/aura-testkit || true
)"
if [[ -n "$legacy_selection_hits" ]]; then
  echo "$legacy_selection_hits" >&2
  fail "legacy non-runtime selection ownership paths must be removed"
fi

if rg -n 'upcoming runtime/app integration|upcoming.*land' \
  crates/aura-agent/src/runtime/services/mod.rs >/dev/null; then
  fail "transitional transparent-envelope scaffolding comments must be removed from runtime service exports"
fi

setup_hits="$(
  rg -n 'TransparentAnonymousSetupLayer|TransparentAnonymousSetupObject' crates \
    | grep -v '^crates/aura-core/src/service.rs:' \
    | grep -v '^crates/aura-core/src/lib.rs:' \
    | grep -v '^crates/aura-agent/src/runtime/services/path_manager.rs:' \
    || true
)"
if [[ -n "$setup_hits" ]]; then
  echo "$setup_hits" >&2
  fail "transparent anonymous setup objects must stay scoped to aura-core service types and the runtime path manager"
fi

for traffic_class in HoldDeposit HoldRetrieval Cover AccountabilityReply; do
  if ! rg -n "TransparentMoveTrafficClass::${traffic_class}" \
    crates/aura-core/src/service.rs >/dev/null; then
    fail "shared transparent move envelope must carry ${traffic_class} traffic"
  fi
done

if ! rg -n 'MoveEnvelope::opaque' \
  crates/aura-agent/src/runtime/services/cover_traffic_generator.rs >/dev/null; then
  fail "cover traffic planning must stay on the shared Move envelope substrate"
fi

if rg -n 'TransportEnvelope' \
  crates/aura-agent/src/runtime/services/cover_traffic_generator.rs >/dev/null; then
  fail "cover traffic planning must not bypass the shared Move envelope substrate"
fi

if rg -n 'TransportHint::|tcp_direct\(|quic_reflexive|fallback_direct_route' \
  crates/aura-agent/src/runtime/services/move_manager.rs \
  crates/aura-agent/src/runtime/services/selection_manager.rs \
  crates/aura-agent/src/runtime/services/cover_traffic_generator.rs >/dev/null; then
  fail "runtime adaptive-privacy services must not reintroduce implicit route setup or direct transport fallback"
fi

for legacy_pattern in 'mailbox polling' 'identity-addressed retrieval' 'direct return channels'; do
  if rg -n "$legacy_pattern" \
    crates/aura-agent/src/runtime/services \
    crates/aura-core/src/service.rs >/dev/null; then
    fail "runtime adaptive-privacy services still reference legacy transport assumption: $legacy_pattern"
  fi
done

echo "adaptive-privacy-phase5-legacy-sweep: ok"
