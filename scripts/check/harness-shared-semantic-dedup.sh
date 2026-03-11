#!/usr/bin/env bash
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

for helper in submit_accept_contact_invitation_via_shared_ui submit_invite_actor_to_channel_via_shared_ui; do
  rg -q "fn $helper" "$contract" || fail "missing shared semantic helper $helper"
done

rg -q 'submit_accept_contact_invitation_via_shared_ui\(self, code\)' "$local_backend" \
  || fail "local backend must route contact invitation acceptance through shared helper"
rg -q 'submit_accept_contact_invitation_via_shared_ui\(self, code\)' "$browser_backend" \
  || fail "browser backend must route contact invitation acceptance through shared helper"
rg -q 'submit_invite_actor_to_channel_via_shared_ui\(self, authority_id\)' "$local_backend" \
  || fail "local backend must route channel invitation through shared helper"
rg -q 'submit_invite_actor_to_channel_via_shared_ui\(self, authority_id\)' "$browser_backend" \
  || fail "browser backend must route channel invitation through shared helper"

if rg -q 'method_name: "submit_create_account"|method_name: "submit_create_home"|method_name: "submit_create_contact_invitation"' "$contract"; then
  :
else
  fail "remaining backend-specific semantic divergences must stay typed and allowlisted"
fi

echo "harness shared semantic dedup: clean"
