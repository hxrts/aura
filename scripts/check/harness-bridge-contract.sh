#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-bridge-contract: $*" >&2
  exit 1
}

ui_contract="crates/aura-app/src/ui_contract.rs"
bridge_impl="crates/aura-web/src/harness_bridge.rs"

extract_bridge_methods() {
  local surface="$1"
  awk -v wanted_surface="$surface" '
    /pub fn install_window_harness_api/ { in_fn=1 }
    in_fn && /^[[:space:]]*&harness,[[:space:]]*$/ { target="harness"; next }
    in_fn && /^[[:space:]]*&observe,[[:space:]]*$/ { target="observe"; next }
    in_fn && target==wanted_surface && match($0, /&JsValue::from_str\("([a-z_]+)"\)/, m) {
      print m[1]
      target=""
    }
    in_fn && /^}$/ { in_fn=0; target="" }
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

mapfile -t contract_methods < <(
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

mapfile -t exported_action_methods < <(extract_bridge_methods harness)
mapfile -t exported_observation_methods < <(extract_bridge_methods observe)
mapfile -t exported_methods < <(
  printf '%s\n' "${exported_action_methods[@]}" "${exported_observation_methods[@]}" \
    | awk 'NF { print }' \
    | rg '^(send_keys|send_key|navigate_screen|snapshot|ui_state|read_clipboard|create_contact_invitation|create_account|create_home|get_authority_id|tail_log|root_structure|inject_message)$' \
    | sort -u
)

if [[ "${contract_methods[*]}" != "${exported_methods[*]}" ]]; then
  printf 'contract methods: %s\n' "${contract_methods[*]}" >&2
  printf 'exported methods: %s\n' "${exported_methods[*]}" >&2
  fail "browser harness bridge metadata does not match exported method surface"
fi

mapfile -t observation_methods < <(
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

rg -q '__AURA_HARNESS_OBSERVE__' "$bridge_impl" \
  || fail "browser observation surface global is not exported"
if printf '%s\n' "${exported_observation_methods[@]}" \
  | rg -q '^(send_keys|send_key|navigate_screen|create_contact_invitation|create_account|create_home|inject_message)$'; then
  fail "browser observation surface exports action methods"
fi

cargo test -p aura-app browser_harness_bridge_contract_is_versioned_and_complete --quiet
cargo test -p aura-app browser_harness_bridge_read_methods_are_declared_deterministic --quiet
cargo test -p aura-app browser_observation_surface_contract_is_versioned_and_read_only --quiet
cargo test -p aura-app tui_observation_surface_contract_is_versioned_and_read_only --quiet
cargo test -p aura-app harness_shell_structure_accepts_exactly_one_app_shell --quiet
cargo test -p aura-app harness_shell_structure_accepts_single_onboarding_shell --quiet
cargo test -p aura-app harness_shell_structure_rejects_duplicate_or_ambiguous_roots --quiet

echo "harness bridge contract: clean"
