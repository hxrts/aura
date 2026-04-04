#!/usr/bin/env bash
# Ensure legacy shared semantic UI helper shortcuts have been removed.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-shared-semantic-dedup: $*" >&2
  exit 1
}

contract="crates/aura-harness/src/backend/mod.rs"
local_backend="crates/aura-harness/src/backend/local_pty.rs"
browser_backend="crates/aura-harness/src/backend/playwright_browser.rs"

if rg -n 'submit_accept_contact_invitation_via_shared_ui|submit_invite_actor_to_channel_via_shared_ui' \
  "$contract" "$local_backend" >/dev/null; then
  fail "legacy shared semantic UI helper shortcuts must be removed"
fi

for command in OpenSettingsSection StartDeviceEnrollment ImportDeviceEnrollmentCode RemoveSelectedDevice \
  CreateContactInvitation InviteActorToChannel SelectChannel; do
  rg -q "HarnessUiCommand::$command" "$local_backend" \
    || fail "local backend must route $command through typed harness commands"
done

if rg -n 'SHARED_INTENT_UI_BYPASS_ALLOWLIST|TemporaryHarnessBridgeShortcut' "$contract" >/dev/null; then
  fail "shared semantic browser bridge migration should remove the old bypass allowlist machinery"
fi

rg -q 'fn submit_semantic_command\(' "$browser_backend" \
  || fail "browser backend must route supported semantic submissions through the typed bridge"
if rg -n 'submit_accept_contact_invitation_via_shared_ui|submit_invite_actor_to_channel_via_shared_ui' "$browser_backend" >/dev/null; then
  fail "browser backend should not keep local-only shared UI helper shortcuts"
fi

echo "harness shared semantic dedup: clean"
