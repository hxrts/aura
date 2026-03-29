#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-bridge-contract: $*" >&2
  exit 1
}

ui_contract="crates/aura-app/src/ui_contract.rs"
bridge_impl="crates/aura-web/src/harness/install.rs"

extract_bridge_methods() {
  local surface="$1"
  awk -v wanted_surface="$surface" '
    /Reflect::set\(/ { in_set=1; target=""; next }
    in_set && /&harness,/ { target="harness"; next }
    in_set && /&observe,/ { target="observe"; next }
    in_set && target==wanted_surface && /&JsValue::from_str\("/ {
      line=$0
      sub(/.*&JsValue::from_str\("/, "", line)
      sub(/".*/, "", line)
      print line
      in_set=0
      target=""
      next
    }
    in_set && /\)\?;$/ { in_set=0; target="" }
  ' "$bridge_impl" | sort -u
}

rg -q 'pub const BROWSER_HARNESS_BRIDGE_API_VERSION' "$ui_contract" \
  || fail "missing browser harness bridge API version"
rg -q 'pub const BROWSER_HARNESS_BRIDGE_METHODS' "$ui_contract" \
  || fail "missing browser harness bridge method metadata"
rg -q 'pub const BROWSER_OBSERVATION_SURFACE_API_VERSION' "$ui_contract" \
  || fail "missing browser observation surface API version"
rg -q 'pub const BROWSER_OBSERVATION_SURFACE_METHODS' "$ui_contract" \
  || fail "missing browser observation surface method metadata"
rg -q 'pub struct HarnessShellStructureSnapshot' "$ui_contract" \
  || fail "missing HarnessShellStructureSnapshot contract"
rg -q 'pub fn validate_harness_shell_structure' "$ui_contract" \
  || fail "missing harness shell structure validator"

contract_methods=()
while IFS= read -r method; do
  [[ -n "$method" ]] || continue
  contract_methods+=("$method")
done < <(
  awk '
    /pub const BROWSER_HARNESS_BRIDGE_METHODS/ { in_block=1; next }
    in_block && /name: "/ {
      name=$0
      sub(/.*name: "/, "", name)
      sub(/".*/, "", name)
      print name
    }
    in_block && /^\];/ { in_block=0 }
  ' "$ui_contract" | sort -u
)

exported_action_methods=()
while IFS= read -r method; do
  [[ -n "$method" ]] || continue
  exported_action_methods+=("$method")
done < <(extract_bridge_methods harness)

exported_observation_methods=()
while IFS= read -r method; do
  [[ -n "$method" ]] || continue
  exported_observation_methods+=("$method")
done < <(extract_bridge_methods observe)

exported_methods=()
while IFS= read -r method; do
  [[ -n "$method" ]] || continue
  exported_methods+=("$method")
done < <(
  printf '%s\n' "${exported_action_methods[@]:-}" "${exported_observation_methods[@]:-}" \
    | awk 'NF { print }' \
    | rg '^(send_keys|send_key|navigate_screen|open_settings_section|snapshot|ui_state|read_clipboard|submit_semantic_command|get_authority_id|tail_log|root_structure|inject_message)$' \
    | sort -u
)

if [[ "${contract_methods[*]}" != "${exported_methods[*]}" ]]; then
  printf 'contract methods: %s\n' "${contract_methods[*]}" >&2
  printf 'exported methods: %s\n' "${exported_methods[*]}" >&2
  fail "browser harness bridge metadata does not match exported method surface"
fi

observation_methods=()
while IFS= read -r method; do
  [[ -n "$method" ]] || continue
  observation_methods+=("$method")
done < <(
  awk '
    /pub const BROWSER_OBSERVATION_SURFACE_METHODS/ { in_block=1; next }
    in_block && /name: "/ {
      name=$0
      sub(/.*name: "/, "", name)
      sub(/".*/, "", name)
      print name
    }
    in_block && /^\];/ { in_block=0 }
  ' "$ui_contract" | sort -u
)

if [[ "${observation_methods[*]}" != "${exported_observation_methods[*]}" ]]; then
  printf 'contract observation methods: %s\n' "${observation_methods[*]}" >&2
  printf 'exported observation methods: %s\n' "${exported_observation_methods[*]}" >&2
  fail "browser observation surface metadata does not match exported observation surface"
fi

rg -q 'HARNESS_OBSERVE_KEY' "$bridge_impl" \
  || fail "browser observation surface global is not exported"
if printf '%s\n' "${exported_observation_methods[@]:-}" \
  | rg -q '^(send_keys|send_key|navigate_screen|submit_semantic_command|inject_message)$'; then
  fail "browser observation surface exports action methods"
fi

cargo test -p aura-app browser_harness_bridge_contract_is_versioned_and_complete --quiet
cargo test -p aura-app browser_harness_bridge_read_methods_are_declared_deterministic --quiet
cargo test -p aura-app browser_observation_surface_contract_is_versioned_and_read_only --quiet
cargo test -p aura-app tui_observation_surface_contract_is_versioned_and_read_only --quiet
cargo test -p aura-app harness_shell_structure_accepts_exactly_one_app_shell --quiet
cargo test -p aura-app harness_shell_structure_accepts_single_onboarding_shell --quiet
cargo test -p aura-app harness_shell_structure_rejects_duplicate_or_ambiguous_roots --quiet
cargo test -p aura-harness playwright_semantic_bridge_failure_and_projection_contracts_are_explicit --quiet

echo "harness bridge contract: clean"
