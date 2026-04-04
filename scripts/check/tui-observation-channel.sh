#!/usr/bin/env bash
# Verify TUI observation channel uses evented snapshots, not filesystem polling.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness tui observation channel: $*" >&2
  exit 1
}

backend="crates/aura-harness/src/backend/local_pty.rs"
ui_snapshot_body="$(perl -0ne 'print $1 if /fn ui_snapshot\(&self\) -> Result<UiSnapshot> \{(.*?)\n\n    fn wait_for_ui_snapshot_event\(/s' "$backend")"
wait_snapshot_body="$(perl -0ne 'print $1 if /fn wait_for_ui_snapshot_event\((.*?)\n\n    fn activate_control\(/s' "$backend")"
bootstrap_wait_body="$(perl -0ne 'print $1 if /fn wait_for_home_bootstrap_ready\((.*?)\n\}\n\nfn home_bootstrap_ready\(/s' crates/aura-harness/src/executor.rs)"

printf '%s\n' "$ui_snapshot_body" | rg -q 'fs::read_to_string|thread::sleep|AURA_TUI_UI_STATE_FILE|SNAPSHOT_WAIT_ATTEMPTS' \
  && fail "local PTY ui_snapshot may not poll the filesystem or sleep"

printf '%s\n' "$wait_snapshot_body" | rg -q 'thread::sleep' \
  && fail "local PTY wait_for_ui_snapshot_event must use the event channel, not sleeps"

printf '%s\n' "$bootstrap_wait_body" | rg -q 'thread::sleep|std::thread::sleep' \
  && fail "home bootstrap wait must not use raw sleep polling"

rg -q 'AURA_TUI_UI_STATE_SOCKET' "$backend" \
  || fail "local PTY backend must provision the TUI snapshot socket"

nix develop -c cargo test -p aura-harness local_backend_uses_socket_driven_ui_snapshot_channel --quiet
nix develop -c cargo test -p aura-harness missing_tui_ui_snapshot_fails_loudly --quiet

echo "harness tui observation channel: clean"
