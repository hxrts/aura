#!/usr/bin/env bash
# Verify ObservationBackend trait does not expose action-like methods.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-observation-surface: $*" >&2
  exit 1
}

backend_mod="crates/aura-harness/src/backend/mod.rs"

rg -q 'pub trait ObservationBackend' "$backend_mod" \
  || fail "missing ObservationBackend trait"

if sed -n '/pub trait ObservationBackend/,/^}/p' "$backend_mod" \
  | rg -q 'fn (send_|click_|fill_|create_|accept_|invite_|join_|inject_|restart|start|stop)'; then
  fail "ObservationBackend exports action-like methods"
fi

cargo test -p aura-app observation_surface_methods_do_not_overlap_action_surface --quiet
cargo test -p aura-harness observation_endpoints_are_side_effect_free --quiet

echo "harness observation surface: clean"
