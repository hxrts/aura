#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-backend-contract: $*" >&2
  exit 1
}

backend_contract="crates/aura-harness/src/backend/mod.rs"
trait_body="$(perl -0ne 'print $1 if /pub trait InstanceBackend \{(.*?)\n\}/s' "$backend_contract")"

[[ -n "$trait_body" ]] || fail "could not extract InstanceBackend trait body"

for forbidden in 'fn click_button' 'fn activate_control' 'fn click_target' 'fn fill_input' 'fn fill_field' 'fn activate_list_item' 'fn submit_create_account' 'fn submit_create_home' 'fn submit_create_contact_invitation' 'fn submit_accept_contact_invitation' 'fn submit_invite_actor_to_channel' 'fn submit_accept_pending_channel_invitation' 'fn submit_join_channel' 'fn submit_send_chat_message'; do
  if grep -Fq "$forbidden" <<<"$trait_body"; then
    fail "InstanceBackend still carries forbidden surface: $forbidden"
  fi
done

for required in 'pub trait ObservationBackend' 'pub trait RawUiBackend' 'pub trait SharedSemanticBackend'; do
  rg -q "$required" "$backend_contract" || fail "missing backend contract surface: $required"
done

if rg -q 'impl<T: InstanceBackend \+ \?Sized> SharedSemanticBackend for T' "$backend_contract"; then
  fail "blanket SharedSemanticBackend impl keeps fallback-heavy semantic execution alive"
fi

rg -q 'impl SharedSemanticBackend for LocalPtyBackend' crates/aura-harness/src/backend/local_pty.rs \
  || fail "local PTY backend must explicitly implement SharedSemanticBackend"
rg -q 'impl SharedSemanticBackend for PlaywrightBrowserBackend' crates/aura-harness/src/backend/playwright_browser.rs \
  || fail "Playwright backend must explicitly implement SharedSemanticBackend"

echo "harness backend contract: clean"
