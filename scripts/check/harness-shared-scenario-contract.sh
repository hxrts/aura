#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "harness-shared-scenario-contract: $*" >&2
  exit 1
}

inventory="scenarios/harness_inventory.toml"
[[ -f "$inventory" ]] || fail "missing inventory: $inventory"

mapfile -t shared_paths < <(
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
  if rg -n 'action\s*=\s*"(press_key|send_key|send_keys|click_button|fill_input|wait_for_dom_patterns|wait_for|expect_toast|expect_command_result|fault_delay)"' "$path" >/tmp/harness-shared-actions.$$ 2>/dev/null; then
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
done

echo "harness shared scenario contract: clean"
