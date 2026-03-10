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
    echo "ux-policy-guardrails: unable to compute diff range; skipping"
    exit 0
  fi
fi

export AURA_UX_POLICY_DIFF_RANGE_RESOLVED="$diff_range"

violations=0
current_file=""
new_line=0

record_violation() {
  local message="$1"
  echo "✖ $message"
  violations=$((violations + 1))
}

is_allowlisted_harness_mode_file() {
  case "$1" in
    crates/aura-app/src/workflows/runtime.rs|\
    crates/aura-app/src/workflows/invitation.rs|\
    crates/aura-agent/src/handlers/invitation.rs|\
    crates/aura-agent/src/runtime/effects.rs|\
    crates/aura-agent/src/runtime_bridge/mod.rs|\
    crates/aura-terminal/src/tui/context/io_context.rs|\
    crates/aura-terminal/src/tui/screens/app/shell.rs|\
    crates/aura-terminal/src/tui/theme.rs)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

is_sleep_guard_path() {
  case "$1" in
    crates/aura-harness/src/backend/*.rs|\
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
    if ! is_allowlisted_harness_mode_file "$current_file"; then
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
    if [[ "$text" == *selected_index* || "$text" == *selected_idx* || "$text" == *selected_by_index* ]]; then
      record_violation "$current_file:$new_line: new row-index selection/addressing in parity-critical export or contract code"
    fi
  fi

  if [[ "$current_file" == scenarios/harness/*.toml ]]; then
    if [[ "$(shared_scenario_classification "$current_file")" == "shared" ]]; then
      if [[ "$text" == *'action = "wait_for"'*
         || "$text" == *'action = "message_contains"'*
         || "$text" == *'action = "expect_toast"'*
         || "$text" == *'action = "expect_command_result"'*
         || "$text" == *'action = "expect_denied"'* ]]; then
        record_violation "$current_file:$new_line: new raw wait or text assertion in shared scenario"
      fi
    fi
  fi

  if [[ "$current_file" != "justfile"
     && "$current_file" != ".github/workflows/ci.yml"
     && "$current_file" != ".github/workflows/harness.yml"
     && "$current_file" != "scripts/check/harness-boundary-policy.sh"
     && "$current_file" != "scripts/check/ux-policy-guardrails.sh"
     && "$current_file" != "scripts/harness/run-matrix.sh"
     && "$current_file" != "docs/804_testing_guide.md"
     && "$current_file" != ".claude/skills/harness-run/SKILL.md" ]]; then
    if [[ "$text" == *'just harness-run'* || "$text" == *'aura-harness -- run'* || "$text" == *'window.__AURA_HARNESS__'* ]]; then
      record_violation "$current_file:$new_line: new frontend-driving harness entry point outside approved owner files"
    fi
  fi

  new_line=$((new_line + 1))
done < <(git diff --unified=0 --no-color "$diff_range")

if [[ "$violations" -gt 0 ]]; then
  echo "ux-policy-guardrails: $violations violation(s)"
  exit 1
fi

echo "ux-policy-guardrails: clean"
