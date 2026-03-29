#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness tui selection contract: $*" >&2
  exit 1
}

rg -q 'fn resolve_committed_selected_channel_id' crates/aura-terminal/src/tui/screens/app/shell/events.rs \
  || fail "missing committed TUI channel selection helper"
rg -q 'SharedCommittedChannelSelection|None::<CommittedChannelSelection>' crates/aura-terminal/src/tui/screens/app/shell.rs \
  || fail "shared TUI channel selection must be tracked by canonical committed channel identity"
if rg -q 'all_channels\(\)[[:space:]]*\.next\(' crates/aura-terminal/src/tui/screens/app/subscriptions.rs; then
  fail "message subscription may not fall back to first channel"
fi

cargo test -p aura-terminal committed_channel_resolution_requires_authoritative_selection --quiet
cargo test -p aura-terminal send_dispatch_does_not_background_retry_selection --quiet
cargo test -p aura-terminal start_chat_dispatch_does_not_optimistically_navigate --quiet
cargo test -p aura-terminal message_subscription_requires_explicit_selected_channel_identity --quiet

echo "harness tui selection contract: clean"
