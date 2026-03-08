#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-core-scenario-mechanics: $*" >&2
  exit 1
}

core_shared_scenarios=(
  "scenarios/harness/scenario12-mixed-device-enrollment-removal-e2e.toml"
  "scenarios/harness/scenario13-mixed-contact-channel-message-e2e.toml"
)

for scenario in "${core_shared_scenarios[@]}"; do
  [[ -f "$scenario" ]] || fail "missing core shared scenario: $scenario"

  if rg -n '^\s*selector\s*=' "$scenario" >/tmp/harness-core-selector.$$ 2>/dev/null; then
    cat /tmp/harness-core-selector.$$ >&2
    rm -f /tmp/harness-core-selector.$$
    fail "raw selector usage is forbidden in core shared scenarios: $scenario"
  fi
  rm -f /tmp/harness-core-selector.$$ || true

  if rg -n '^\s*label\s*=' "$scenario" >/tmp/harness-core-label.$$ 2>/dev/null; then
    cat /tmp/harness-core-label.$$ >&2
    rm -f /tmp/harness-core-label.$$
    fail "label-based click targeting is forbidden in core shared scenarios: $scenario"
  fi
  rm -f /tmp/harness-core-label.$$ || true

  if rg -n 'action\s*=\s*"(press_key|send_key|send_keys|click_button|expect_toast|expect_command_result|wait_for_dom_patterns)"' "$scenario" >/tmp/harness-core-actions.$$ 2>/dev/null; then
    cat /tmp/harness-core-actions.$$ >&2
    rm -f /tmp/harness-core-actions.$$
    fail "raw mechanics action is forbidden in core shared scenarios: $scenario"
  fi
  rm -f /tmp/harness-core-actions.$$ || true

  if rg -n '^\s*screen_source\s*=\s*"dom"' "$scenario" >/tmp/harness-core-dom.$$ 2>/dev/null; then
    cat /tmp/harness-core-dom.$$ >&2
    rm -f /tmp/harness-core-dom.$$
    fail "DOM-specific observation overrides are forbidden in core shared scenarios: $scenario"
  fi
  rm -f /tmp/harness-core-dom.$$ || true
done

echo "harness core scenario mechanics: clean"
