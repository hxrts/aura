#!/usr/bin/env bash
# Ensure adaptive-privacy selection profiles remain runtime-local.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "adaptive-privacy-runtime-locality: $*" >&2
  exit 1
}

selection_manager_file="crates/aura-agent/src/runtime/services/selection_manager.rs"
registry_file="crates/aura-agent/src/runtime/services/service_registry.rs"
agent_arch="crates/aura-agent/ARCHITECTURE.md"

if rg -n 'authoritative = ".*LocalSelectionProfile' "$selection_manager_file" >/dev/null; then
  fail "LocalSelectionProfile must remain runtime-local, not an authoritative service-surface object"
fi

if ! rg -n 'authoritative = ""' "$selection_manager_file" >/dev/null; then
  fail "selection_manager service_surface must declare an empty authoritative set"
fi

if ! rg -n 'runtime_local = ".*selection_profiles.*"' "$selection_manager_file" >/dev/null; then
  fail "selection_manager service_surface must declare selection profiles as runtime-local state"
fi

authoritative_uses="$(
  rg -n 'LocalSelectionProfile' \
    crates/aura-agent/src \
    crates/aura-app/src \
    crates/aura-terminal/src \
    crates/aura-web/src \
    crates/aura-harness/src \
    -g '!crates/aura-agent/src/runtime/services/selection_manager.rs' \
    -g '!crates/aura-agent/src/runtime/services/mod.rs' \
    -g '!crates/aura-agent/src/lib.rs' || true
)"
if [[ -n "$authoritative_uses" ]]; then
  echo "$authoritative_uses" >&2
  fail "LocalSelectionProfile must not escape the runtime-owned selection service surface"
fi

if ! rg -n 'SelectionState' "$registry_file" >/dev/null; then
  fail "service_registry must store SelectionState snapshots for sanctioned runtime-local queries"
fi

if ! rg -n 'Adaptive privacy runtime-owned services include `SelectionManager`, `LocalHealthObserver`, `CoverTrafficGenerator`, and `AnonymousPathManager`' "$agent_arch" >/dev/null; then
  fail "aura-agent ARCHITECTURE.md must document the adaptive privacy runtime-owned service set"
fi

if ! rg -n '`LocalSelectionProfile` is runtime-local' "$agent_arch" >/dev/null; then
  fail "aura-agent ARCHITECTURE.md must state that LocalSelectionProfile is runtime-local"
fi

echo "adaptive-privacy-runtime-locality: ok"
