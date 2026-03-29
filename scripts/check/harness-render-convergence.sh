#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-render-convergence: $*" >&2
  exit 1
}

rg -q 'fn publish_ui_snapshot' crates/aura-web/src/harness_bridge.rs \
  || fail "missing web publish hook"
rg -q 'requestAnimationFrame' crates/aura-web/src/harness/publication.rs \
  || fail "web publish hook must go through requestAnimationFrame"
rg -q 'publish_render_heartbeat' crates/aura-web/src/harness/publication.rs \
  || fail "web publish hook must emit render heartbeat"
rg -q 'next_projection_revision' crates/aura-terminal/src/tui/harness_state/snapshot.rs \
  || fail "tui semantic snapshots must publish projection revisions"

cargo test -p aura-app render_convergence_accepts_matching_snapshot_and_heartbeat --quiet
cargo test -p aura-app render_convergence_rejects_semantic_state_published_ahead_of_renderer --quiet

echo "harness render convergence: clean"
