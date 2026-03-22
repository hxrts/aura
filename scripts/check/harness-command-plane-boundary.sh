#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-command-plane-boundary: $*" >&2
  exit 1
}

allowed_rust_files=(
  "crates/aura-harness/src/backend/mod.rs"
  "crates/aura-harness/src/backend/local_pty.rs"
  "crates/aura-harness/src/backend/playwright_browser.rs"
  "crates/aura-harness/src/tool_api.rs"
  "crates/aura-harness/src/coordinator.rs"
  "crates/aura-harness/src/executor.rs"
  "crates/aura-web/src/harness_bridge.rs"
)

allowed_ts_files=(
  "crates/aura-harness/playwright-driver/src/playwright_driver.ts"
  "crates/aura-harness/playwright-driver/src/contracts.ts"
  "crates/aura-harness/playwright-driver/src/method_sets.ts"
)

rust_hits=()
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  rust_hits+=("$file")
done < <(
  rg -l 'fn submit_semantic_command\(|submit_semantic_command_via_ui\(' \
    crates/aura-harness crates/aura-web 2>/dev/null | sort -u
)

for file in "${rust_hits[@]}"; do
  if [[ ! " ${allowed_rust_files[*]} " =~ " ${file} " ]]; then
    fail "unexpected semantic command handling surface in Rust module: $file"
  fi
done

ts_hits=()
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  ts_hits+=("$file")
done < <(
  rg -l 'submit_semantic_command' crates/aura-harness/playwright-driver 2>/dev/null | sort -u
)

for file in "${ts_hits[@]}"; do
  if [[ ! " ${allowed_ts_files[*]} " =~ " ${file} " ]]; then
    fail "unexpected semantic command handling surface in Playwright driver: $file"
  fi
done

rg -q 'tool_api.submit_semantic_command\(instance_id, SemanticCommandRequest::new\(intent\)\)' \
  crates/aura-harness/src/executor.rs \
  || fail "executor must submit shared intents only through ToolApi::submit_semantic_command"

if rg -n 'submit_create_account\(|submit_create_home\(|submit_create_contact_invitation\(|submit_accept_contact_invitation\(|submit_invite_actor_to_channel\(|submit_accept_pending_channel_invitation\(|submit_join_channel\(|submit_send_chat_message\(' \
  crates/aura-harness/src/tool_api.rs >/tmp/harness-command-plane-boundary.$$ 2>/dev/null; then
  cat /tmp/harness-command-plane-boundary.$$ >&2
  rm -f /tmp/harness-command-plane-boundary.$$ || true
  fail "per-intent semantic command wrappers must not reappear in ToolApi"
fi
rm -f /tmp/harness-command-plane-boundary.$$ || true

if rg -n 'create_account_via_ui\(|create_home_via_ui\(|create_contact_invitation_via_ui\(|accept_contact_invitation_via_ui\(|invite_actor_to_channel_via_ui\(|accept_pending_channel_invitation_via_ui\(|join_channel_via_ui\(|send_chat_message_via_ui\(' \
  crates/aura-harness/src/coordinator.rs >/tmp/harness-command-plane-boundary.$$ 2>/dev/null; then
  cat /tmp/harness-command-plane-boundary.$$ >&2
  rm -f /tmp/harness-command-plane-boundary.$$ || true
  fail "per-intent semantic command wrappers must not reappear in HarnessCoordinator"
fi
rm -f /tmp/harness-command-plane-boundary.$$ || true

echo "harness command-plane boundary: clean"
