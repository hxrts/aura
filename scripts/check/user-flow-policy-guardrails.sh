#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

diff_range="${AURA_UX_POLICY_DIFF_RANGE:-}"
if [[ -z "$diff_range" ]]; then
  if [[ -n "${GITHUB_BASE_REF:-}" ]] && git rev-parse --verify "origin/${GITHUB_BASE_REF}" >/dev/null 2>&1; then
    diff_range="origin/${GITHUB_BASE_REF}...HEAD"
  elif git rev-parse --verify HEAD >/dev/null 2>&1; then
    diff_range="HEAD"
  else
    echo "user-flow-policy-guardrails: unable to compute diff range; skipping"
    exit 0
  fi
fi

export AURA_UX_POLICY_DIFF_RANGE_RESOLVED="$diff_range"

changed_files="$(git diff --name-only "$diff_range" || true)"

changed_list=()
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  changed_list+=("$file")
done <<< "$changed_files"

violations=0
current_file=""
new_line=0

record_violation() {
  local message="$1"
  echo "✖ $message"
  violations=$((violations + 1))
}

has_changed() {
  local target="$1"
  local file
  for file in "${changed_list[@]}"; do
    [[ "$file" == "$target" ]] && return 0
  done
  return 1
}

is_allowlisted_harness_mode_file() {
  case "$1" in
    crates/aura-app/src/workflows/runtime.rs|\
    crates/aura-app/src/workflows/invitation.rs|\
    crates/aura-agent/src/handlers/invitation.rs|\
    crates/aura-agent/src/runtime/effects.rs|\
    crates/aura-agent/src/runtime_bridge/mod.rs|\
    crates/aura-terminal/src/tui/context/io_context.rs|\
    crates/aura-web/src/main.rs)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

allowlisted_harness_mode_owner() {
  case "$1" in
    crates/aura-app/src/workflows/runtime.rs) echo "aura-app-runtime" ;;
    crates/aura-app/src/workflows/invitation.rs) echo "aura-app-invitation" ;;
    crates/aura-agent/src/handlers/invitation.rs) echo "aura-agent-invitation" ;;
    crates/aura-agent/src/runtime/effects.rs) echo "aura-agent-runtime-effects" ;;
    crates/aura-agent/src/runtime_bridge/mod.rs) echo "aura-agent-runtime-bridge" ;;
    crates/aura-terminal/src/tui/context/io_context.rs) echo "aura-terminal-tui-context" ;;
    crates/aura-web/src/main.rs) echo "aura-web-main" ;;
  esac
}

allowlisted_harness_mode_justification() {
  case "$1" in
    crates/aura-app/src/workflows/runtime.rs) echo "runtime harness toggles and deterministic instrumentation only" ;;
    crates/aura-app/src/workflows/invitation.rs) echo "invitation harness instrumentation only; no parity-critical flow bypass" ;;
    crates/aura-agent/src/handlers/invitation.rs) echo "runtime-owned invitation handler instrumentation only" ;;
    crates/aura-agent/src/runtime/effects.rs) echo "effect wiring for deterministic harness-mode runtime assembly only" ;;
    crates/aura-agent/src/runtime_bridge/mod.rs) echo "runtime bridge instrumentation and environment binding only" ;;
    crates/aura-terminal/src/tui/context/io_context.rs) echo "TUI IO instrumentation and deterministic harness plumbing only" ;;
    crates/aura-web/src/main.rs) echo "web harness instrumentation and snapshot publication only" ;;
  esac
}

allowlisted_harness_mode_design_ref() {
  case "$1" in
    crates/aura-app/src/workflows/runtime.rs) echo "docs/804_testing_guide.md" ;;
    crates/aura-app/src/workflows/invitation.rs) echo "docs/804_testing_guide.md" ;;
    crates/aura-agent/src/handlers/invitation.rs) echo "docs/804_testing_guide.md" ;;
    crates/aura-agent/src/runtime/effects.rs) echo "docs/804_testing_guide.md" ;;
    crates/aura-agent/src/runtime_bridge/mod.rs) echo "docs/804_testing_guide.md" ;;
    crates/aura-terminal/src/tui/context/io_context.rs) echo "crates/aura-terminal/ARCHITECTURE.md" ;;
    crates/aura-web/src/main.rs) echo "docs/804_testing_guide.md" ;;
  esac
}

validate_allowlisted_harness_mode_metadata() {
  local file owner justification design_ref
  for file in \
    crates/aura-app/src/workflows/runtime.rs \
    crates/aura-app/src/workflows/invitation.rs \
    crates/aura-agent/src/handlers/invitation.rs \
    crates/aura-agent/src/runtime/effects.rs \
    crates/aura-agent/src/runtime_bridge/mod.rs \
    crates/aura-terminal/src/tui/context/io_context.rs \
    crates/aura-web/src/main.rs; do
    owner="$(allowlisted_harness_mode_owner "$file")"
    justification="$(allowlisted_harness_mode_justification "$file")"
    design_ref="$(allowlisted_harness_mode_design_ref "$file")"

    [[ -n "$owner" ]] || record_violation "$file: missing allowlisted harness-mode owner metadata"
    [[ -n "$justification" ]] || record_violation "$file: missing allowlisted harness-mode justification metadata"
    [[ -n "$design_ref" ]] || record_violation "$file: missing allowlisted harness-mode design-note reference"
    if [[ -n "$design_ref" && ! -e "$design_ref" ]]; then
      record_violation "$file: allowlisted harness-mode design-note reference does not exist: $design_ref"
    fi
  done
}

is_sleep_guard_path() {
  case "$1" in
    crates/aura-harness/src/coordinator.rs|\
    crates/aura-harness/src/executor.rs|\
    crates/aura-harness/playwright-driver/playwright_driver.mjs|\
    crates/aura-terminal/src/tui/harness_state.rs|\
    crates/aura-web/src/harness_bridge.rs)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

is_row_index_guard_path() {
  case "$1" in
    crates/aura-app/src/ui_contract.rs|\
    crates/aura-terminal/src/tui/harness_state.rs)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

is_allowlisted_harness_entrypoint_file() {
  case "$1" in
    justfile|\
    .github/workflows/ci.yml|\
    .github/workflows/harness.yml|\
    scripts/check/harness-boundary-policy.sh|\
    scripts/check/user-flow-policy-guardrails.sh|\
    scripts/harness/run-matrix.sh|\
    docs/804_testing_guide.md|\
    .claude/skills/harness-run/SKILL.md)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

is_shared_scenario_allowed_action() {
  case "$1" in
    launch_actors|\
    screen_is|\
    create_account|\
    readiness_is|\
    open_screen|\
    start_device_enrollment|\
    runtime_event_occurred|\
    import_device_enrollment_code|\
    open_settings_section|\
    selection_is|\
    list_count_is|\
    remove_selected_device|\
    capture_current_authority_id|\
    create_contact_invitation|\
    accept_contact_invitation|\
    join_channel|\
    invite_actor_to_channel|\
    accept_pending_channel_invitation|\
    send_chat_message|\
    parity_with_actor)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

shared_scenario_classification() {
  local scenario_path="$1"
  awk -v target="$scenario_path" '
    BEGIN { path=""; class="" }
    /^\[\[scenario\]\]/ {
      if (path == target) {
        print class
        exit
      }
      path=""
      class=""
      next
    }
    /^path = / {
      gsub(/^path = |"/, "", $0)
      path=$0
      next
    }
    /^classification = / {
      gsub(/^classification = |"/, "", $0)
      class=$0
      next
    }
    END {
      if (path == target) print class
    }
  ' scenarios/harness_inventory.toml
}

require_browser_bridge_compat_updates() {
  local bridge_changed=false
  if has_changed "crates/aura-web/src/harness_bridge.rs" \
    || has_changed "crates/aura-web/src/main.rs" \
    || has_changed "crates/aura-harness/playwright-driver/playwright_driver.mjs"; then
    bridge_changed=true
  fi

  if ! $bridge_changed; then
    return
  fi

  if ! has_changed "crates/aura-web/ARCHITECTURE.md"; then
    record_violation "browser harness bridge compatibility changes require crates/aura-web/ARCHITECTURE.md updates"
  fi

  if ! has_changed "docs/804_testing_guide.md"; then
    record_violation "browser harness bridge compatibility changes require docs/804_testing_guide.md updates"
  fi
}

validate_allowlisted_harness_mode_metadata
require_browser_bridge_compat_updates

while IFS= read -r raw_line; do
  if [[ "$raw_line" == "+++ b/"* ]]; then
    current_file="${raw_line#+++ b/}"
    continue
  fi

  if [[ "$raw_line" == "+++ /dev/null" ]]; then
    current_file=""
    continue
  fi

  if [[ "$raw_line" =~ ^@@[[:space:]]-[0-9,]+[[:space:]]\+([0-9]+) ]]; then
    new_line="${BASH_REMATCH[1]}"
    continue
  fi

  if [[ -z "$current_file" || "$raw_line" != +* || "$raw_line" == "+++"* ]]; then
    continue
  fi

  text="${raw_line:1}"

  if [[ "$text" == *"AURA_HARNESS_MODE"* && "$current_file" == crates/* && "$current_file" != crates/aura-harness/* ]]; then
    if [[ "$text" != *'contains("AURA_HARNESS_MODE")'* && "$text" != *"assert!(!"* ]] \
      && ! is_allowlisted_harness_mode_file "$current_file"; then
      record_violation "$current_file:$new_line: new AURA_HARNESS_MODE branch outside allowlisted instrumentation surface"
    fi
  fi

  if is_sleep_guard_path "$current_file"; then
    if [[ "$text" == *thread::sleep* || "$text" == *std::thread::sleep* || "$text" == *tokio::time::sleep* || "$text" == *recv_timeout* || "$text" == *POLL_INTERVAL* || "$text" == *poll_interval* ]]; then
      record_violation "$current_file:$new_line: new sleep/polling helper in parity-critical harness or export path"
    fi
  fi

  if [[ "$current_file" == crates/* ]]; then
    if [[ "$text" == *normalize_parity_* || "$text" == *parity*normalize* || "$text" == *parity*remap* || "$text" == *normalize*parity* || "$text" == *remap*parity* ]]; then
      record_violation "$current_file:$new_line: new parity remap/normalization helper"
    fi
  fi

  if [[ "$current_file" == crates/* && "$current_file" != "crates/aura-app/src/ui_contract.rs" ]]; then
    if [[ "$text" == *'ScreenId("'*
       || "$text" == *'ModalId("'*
       || "$text" == *'ControlId("'*
       || "$text" == *'FieldId("'*
       || "$text" == *'ListId("'*
       || "$text" == *'OperationId("'*
       || "$text" == *'RuntimeEventId("'*
       || "$text" == *'ToastId("'* ]]; then
      record_violation "$current_file:$new_line: new stringly-typed parity identifier outside aura-app::ui_contract"
    fi
  fi

  if is_row_index_guard_path "$current_file"; then
    if [[ "$text" == *selected_idx* || "$text" == *selected_by_index* || "$text" == *selected_channel_index* ]]; then
      record_violation "$current_file:$new_line: new row-index selection/addressing in parity-critical export or contract code"
    fi
  fi

  if [[ "$current_file" == scenarios/harness/*.toml ]]; then
    if [[ "$text" == schema_version\ *=* || "$text" == execution_mode\ *=* || "$text" == required_capabilities\ *=* ]]; then
      record_violation "$current_file:$new_line: new legacy scenario-dialect field in scenarios/harness"
    fi

    if [[ "$(shared_scenario_classification "$current_file")" == "shared" ]]; then
      if [[ "$text" =~ ^action\ =\ \"([^\"]+)\"$ ]]; then
        action_name="${BASH_REMATCH[1]}"
        if ! is_shared_scenario_allowed_action "$action_name"; then
          record_violation "$current_file:$new_line: new non-semantic or non-allowlisted action '$action_name' in shared scenario"
        fi
      fi
    fi
  fi

  if ! is_allowlisted_harness_entrypoint_file "$current_file"; then
    if [[ "$current_file" != scripts/check/* ]] && [[ "$text" == *'just harness-run'* \
       || "$text" == *'just harness-run-browser'* \
       || "$text" == *'aura-harness -- run'* \
       || "$text" == *'cargo run -p aura-harness --bin aura-harness'* \
       || "$text" == *'window.__AURA_HARNESS__'* ]]; then
      record_violation "$current_file:$new_line: new frontend-driving harness entry point outside approved owner files"
    fi
  fi

  if [[ "$current_file" == crates/aura-terminal/src/tui/* || "$current_file" == crates/aura-terminal/src/handlers/tui.rs ]]; then
    if [[ "$text" == *'screen_id: String'* \
       || "$text" == *'modal_id: String'* \
       || "$text" == *'control_id: String'* \
       || "$text" == *'field_id: String'* \
       || "$text" == *'list_id: String'* \
       || "$text" == *'operation_id: String'* ]]; then
      record_violation "$current_file:$new_line: new parity-critical TUI surface uses raw String identifier"
    fi
  fi

  new_line=$((new_line + 1))
done < <(git diff --unified=0 --no-color "$diff_range")

if [[ "$violations" -gt 0 ]]; then
  echo "user-flow-policy-guardrails: $violations violation(s)"
  exit 1
fi

echo "user-flow-policy-guardrails: clean"
