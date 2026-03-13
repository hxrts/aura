#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-boundary-policy: $*" >&2
  exit 1
}

forbidden_contract_patterns='send_keys|send_key|click_button|click_target|fill_input|selector|dom_snapshot'
if rg -n "$forbidden_contract_patterns" crates/aura-app/src/scenario_contract.rs \
  | rg -v ':\s*//!|:\s*//|:\s*\*' >/tmp/harness-contract-forbidden.$$ 2>/dev/null; then
  cat /tmp/harness-contract-forbidden.$$ >&2
  rm -f /tmp/harness-contract-forbidden.$$
  fail "semantic scenario contract contains frontend-specific mechanics"
fi
rm -f /tmp/harness-contract-forbidden.$$ || true

forbidden_quint_patterns='aura_terminal|aura_ui|playwright|ToolRequest|send_keys|send_key|click_button|fill_input'
if rg -n "$forbidden_quint_patterns" crates/aura-quint verification/quint \
  | rg -v ':\s*//!|:\s*//|:\s*\*' >/tmp/harness-quint-forbidden.$$ 2>/dev/null; then
  cat /tmp/harness-quint-forbidden.$$ >&2
  rm -f /tmp/harness-quint-forbidden.$$
  fail "Quint or verification code references frontend-driving mechanics"
fi
rm -f /tmp/harness-quint-forbidden.$$ || true

active_ref_roots=(
  justfile
  scripts/ci
  crates/aura-harness
  docs/804_testing_guide.md
)

mapfile -t referenced_scenarios < <(
  rg -o --no-filename 'scenarios/harness/[A-Za-z0-9._/-]+\.toml' "${active_ref_roots[@]}" \
    | sort -u
)

for scenario in "${referenced_scenarios[@]}"; do
  [[ -f "$scenario" ]] || fail "active entry point references missing scenario: $scenario"
  if rg -q '^(schema_version|execution_mode|required_capabilities)\s*=' "$scenario"; then
    fail "active entry point references legacy harness scenario: $scenario"
  fi
done

mapfile -t semantic_scenarios < <(
  find scenarios/harness -maxdepth 1 -name '*.toml' | sort | while read -r scenario; do
    if ! rg -q '^(schema_version|execution_mode|required_capabilities)\s*=' "$scenario"; then
      printf '%s\n' "$scenario"
    fi
  done
)

for scenario in "${semantic_scenarios[@]}"; do
  if rg -q '^\s*selector\s*=' "$scenario"; then
    fail "semantic scenario contains raw selector reference: $scenario"
  fi
  if rg -q 'action\s*=\s*"(wait_for|click_button|fill_input|send_keys|send_key)"' "$scenario"; then
    fail "semantic scenario contains legacy frontend action: $scenario"
  fi
done

echo "harness boundary policy: clean"
