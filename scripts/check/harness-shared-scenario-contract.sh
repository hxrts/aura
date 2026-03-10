#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "harness-shared-scenario-contract: $*" >&2
  exit 1
}

inventory="scenarios/harness_inventory.toml"
[[ -f "$inventory" ]] || fail "missing inventory: $inventory"
allowed_actions='launch_actors|restart_actor|kill_actor|fault_delay|fault_loss|fault_tunnel_drop|open_screen|create_account|start_device_enrollment|import_device_enrollment_code|open_settings_section|remove_selected_device|create_contact_invitation|accept_contact_invitation|accept_pending_channel_invitation|join_channel|invite_actor_to_channel|send_chat_message|screen_is|modal_open|message_contains|toast_contains|list_contains|list_count_is|list_item_confirmation|selection_is|readiness_is|runtime_event_occurred|operation_state_is|parity_with_actor|capture_current_authority_id|capture_selection|set_var|extract_var'

shared_paths=()
while IFS= read -r line; do
  shared_paths+=("$line")
done < <(
  awk '
    /^\[\[scenario\]\]/ { path=""; class="" }
    /^path = / { gsub(/^path = "|"$/, "", $0); path=$0; sub(/^path = /, "", path); gsub(/"/, "", path) }
    /^classification = / { class=$3; gsub(/"/, "", class) }
    /^migration_status = / { status=$3; gsub(/"/, "", status) }
    /^notes = / {
      if (class == "shared") print path "|" status
    }
  ' "$inventory" | while IFS='|' read -r path status; do
    printf '%s|%s\n' "$path" "$status"
  done
)

[[ ${#shared_paths[@]} -gt 0 ]] || fail "no shared scenarios found in inventory"

for entry in "${shared_paths[@]}"; do
  path=${entry%%|*}
  status=${entry##*|}
  [[ "$status" == "converted" ]] || fail "shared scenario is not converted: $path ($status)"
  [[ -f "$path" ]] || fail "missing shared scenario: $path"
  if rg -q '^(schema_version|execution_mode|required_capabilities)\s*=' "$path"; then
    fail "shared scenario still uses legacy harness schema: $path"
  fi
  if rg '^\s*action\s*=\s*"([^"]+)"' -or '$1' "$path" | rg -vx "$allowed_actions" >/tmp/harness-shared-bad-action.$$ 2>/dev/null; then
    cat /tmp/harness-shared-bad-action.$$ >&2
    rm -f /tmp/harness-shared-bad-action.$$
    fail "shared scenario contains action outside the intent contract: $path"
  fi
  rm -f /tmp/harness-shared-bad-action.$$ || true
  if rg -n '^\s*control_id\s*=' "$path" >/tmp/harness-shared-control.$$ 2>/dev/null; then
    cat /tmp/harness-shared-control.$$ >&2
    rm -f /tmp/harness-shared-control.$$
    fail "shared scenario contains raw control targeting: $path"
  fi
  rm -f /tmp/harness-shared-control.$$ || true
  if rg -n '^\s*field_id\s*=' "$path" >/tmp/harness-shared-field.$$ 2>/dev/null; then
    cat /tmp/harness-shared-field.$$ >&2
    rm -f /tmp/harness-shared-field.$$
    fail "shared scenario contains raw field targeting: $path"
  fi
  rm -f /tmp/harness-shared-field.$$ || true
  if rg -n '^\s*selector\s*=' "$path" >/tmp/harness-shared-selector.$$ 2>/dev/null; then
    cat /tmp/harness-shared-selector.$$ >&2
    rm -f /tmp/harness-shared-selector.$$
    fail "shared scenario contains raw selector reference: $path"
  fi
  rm -f /tmp/harness-shared-selector.$$ || true
  if rg -n '^\s*label\s*=' "$path" >/tmp/harness-shared-label.$$ 2>/dev/null; then
    cat /tmp/harness-shared-label.$$ >&2
    rm -f /tmp/harness-shared-label.$$
    fail "shared scenario contains label-based targeting: $path"
  fi
  rm -f /tmp/harness-shared-label.$$ || true
  if rg -n 'action\s*=\s*"(navigate|activate|activate_list_item|fill|input_text|dismiss_transient|press_key|send_key|send_keys|click_button|fill_input|read_clipboard|send_clipboard|wait_for_dom_patterns|wait_for|expect_toast|expect_command_result|control_visible|send_chat_command)"' "$path" >/tmp/harness-shared-actions.$$ 2>/dev/null; then
    cat /tmp/harness-shared-actions.$$ >&2
    rm -f /tmp/harness-shared-actions.$$
    fail "shared scenario contains raw mechanics action: $path"
  fi
  rm -f /tmp/harness-shared-actions.$$ || true
  if rg -n '^\s*pattern\s*=' "$path" >/tmp/harness-shared-pattern.$$ 2>/dev/null; then
    cat /tmp/harness-shared-pattern.$$ >&2
    rm -f /tmp/harness-shared-pattern.$$
    fail "shared scenario contains raw text assertion: $path"
  fi
  rm -f /tmp/harness-shared-pattern.$$ || true
  if rg -n '^\s*screen_source\s*=\s*"dom"' "$path" >/tmp/harness-shared-dom.$$ 2>/dev/null; then
    cat /tmp/harness-shared-dom.$$ >&2
    rm -f /tmp/harness-shared-dom.$$
    fail "shared scenario overrides observation to raw DOM path: $path"
  fi
  rm -f /tmp/harness-shared-dom.$$ || true
  if rg -q 'action\s*=\s*"start_device_enrollment"' "$path" \
    && ! rg -q 'runtime_event_kind\s*=\s*"device_enrollment_code_ready"' "$path"; then
    fail "shared scenario uses start_device_enrollment without device_enrollment_code_ready barrier: $path"
  fi
  if rg -q 'action\s*=\s*"create_contact_invitation"' "$path" \
    && ! rg -q 'runtime_event_kind\s*=\s*"invitation_code_ready"' "$path"; then
    fail "shared scenario uses create_contact_invitation without invitation_code_ready barrier: $path"
  fi
  if rg -q 'action\s*=\s*"accept_contact_invitation"' "$path" \
    && ! rg -q 'runtime_event_kind\s*=\s*"contact_link_ready"' "$path"; then
    fail "shared scenario uses accept_contact_invitation without contact_link_ready barrier: $path"
  fi
  if rg -q 'action\s*=\s*"join_channel"' "$path" \
    && ! rg -q 'runtime_event_kind\s*=\s*"channel_membership_ready"' "$path"; then
    fail "shared scenario uses join_channel without channel_membership_ready barrier: $path"
  fi
  if rg -q 'action\s*=\s*"send_chat_message"' "$path" \
    && ! rg -q 'runtime_event_kind\s*=\s*"recipient_peers_resolved"' "$path"; then
    fail "shared scenario uses send_chat_message without recipient_peers_resolved barrier: $path"
  fi
done

echo "harness shared scenario contract: clean"
