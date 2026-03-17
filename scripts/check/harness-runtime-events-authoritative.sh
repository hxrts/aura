#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness runtime events authoritative: $*" >&2
  exit 1
}

production_source="$(awk '/#\[cfg\(test\)\]/{exit} {print}' crates/aura-terminal/src/tui/harness_state/snapshot.rs)"

if printf '%s\n' "$production_source" | rg -q 'RuntimeFact::(ContactLinkReady|PendingHomeInvitationReady|ChannelMembershipReady|RecipientPeersResolved|MessageDeliveryReady)'; then
  fail "TUI snapshot export may not synthesize parity-critical runtime facts"
fi

if printf '%s\n' "$production_source" | rg -F -q 'runtime_events.push(RuntimeEventSnapshot'; then
  fail "TUI snapshot export may not append runtime events heuristically"
fi

cargo test -p aura-terminal semantic_snapshot_exports_tui_owned_runtime_facts --quiet
cargo test -p aura-terminal semantic_snapshot_exporter_does_not_infer_parity_runtime_events --quiet
cargo test -p aura-app ui_snapshot_parity_detects_runtime_event_shape_drift --quiet
cargo test -p aura-app ui_snapshot_parity_detects_toast_drift --quiet
cargo test -p aura-harness shared_intent_waits_bind_only_to_declared_barriers --quiet

echo "harness runtime events authoritative: clean"
