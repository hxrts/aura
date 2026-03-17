#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness tui semantic snapshot: $*" >&2
  exit 1
}

if rg -q '^static (CONTACTS_OVERRIDE|DEVICES_OVERRIDE|MESSAGES_OVERRIDE)' crates/aura-terminal/src/tui/harness_state/snapshot.rs; then
  fail "parity-critical TUI exporter may not use contact/device/message override caches"
fi

if rg -q '^pub fn (publish_contacts_list_export|publish_devices_list_export|publish_messages_export)' \
  crates/aura-terminal/src/tui/harness_state/snapshot.rs; then
  fail "parity-critical TUI exporter may not declare contact/device/message publish overrides"
fi

if rg -q 'publish_contacts_list_export|publish_devices_list_export|publish_messages_export' \
  crates/aura-terminal/src/tui/screens \
  crates/aura-terminal/src/tui/screens/app/subscriptions.rs; then
  fail "parity-critical TUI exporter may not depend on contact/device/message publish overrides"
fi

rg -q 'pub struct TuiSemanticInputs' crates/aura-terminal/src/tui/harness_state/commands.rs \
  || fail "missing explicit TUI semantic input contract"
rg -q 'exported_runtime_events' crates/aura-terminal/src/tui/harness_state/snapshot.rs \
  || fail "TUI exporter must consume runtime facts from owned state"

cargo test -p aura-terminal semantic_snapshot_does_not_synthesize_placeholder_contact_ids --quiet
cargo test -p aura-terminal semantic_snapshot_exporter_does_not_depend_on_parity_override_caches --quiet
cargo test -p aura-terminal semantic_snapshot_ready_state_is_projection_only --quiet
cargo test -p aura-terminal semantic_snapshot_exports_tui_owned_runtime_facts --quiet

echo "harness tui semantic snapshot: clean"
