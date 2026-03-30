#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "service-registry-ownership: $*" >&2
  exit 1
}

if [[ -e crates/aura-agent/src/runtime/services/rendezvous_cache_manager.rs ]]; then
  fail "legacy rendezvous_cache_manager.rs must be removed"
fi

if ! rg -n '#\[aura_macros::actor_owned\(' crates/aura-agent/src/runtime/services/service_registry.rs >/dev/null; then
  fail "service_registry.rs must declare the actor-owned registry service"
fi

if rg -n 'RendezvousCacheManager|pending_channels|descriptor_cache' \
  crates/aura-agent/src crates/aura-sync/src crates/aura-rendezvous/src; then
  fail "legacy duplicate rendezvous cache ownership paths are still present"
fi

duplicate_descriptor_stores="$(
  rg -n 'HashMap<\(\s*ContextId,\s*AuthorityId\s*\),\s*RendezvousDescriptor>' \
    crates/aura-agent/src/runtime/services \
    -g '!service_registry.rs' \
    -g '!rendezvous_manager.rs' || true
)"
if [[ -n "$duplicate_descriptor_stores" ]]; then
  echo "$duplicate_descriptor_stores" >&2
  fail "duplicate runtime descriptor stores detected outside service_registry/rendezvous_manager"
fi

echo "service-registry-ownership: ok"
