#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

contract_file="crates/aura-app/src/ui_contract.rs"

fail() {
  echo "ui parity contract: $*" >&2
  exit 1
}

tmp_variants="$(mktemp)"
tmp_metadata="$(mktemp)"
tmp_docs="$(mktemp)"
cleanup() {
  rm -f "$tmp_variants" "$tmp_metadata" "$tmp_docs"
}
trap cleanup EXIT

awk '
  /pub enum ParityException/ { in_enum = 1; next }
  in_enum && /^\}/ { in_enum = 0; exit }
  in_enum {
    if (match($0, /^[[:space:]]*([A-Za-z0-9_]+),[[:space:]]*$/, m)) {
      print m[1]
    }
  }
' "$contract_file" | sort -u > "$tmp_variants"

rg -o 'exception: ParityException::[A-Za-z0-9_]+' "$contract_file" \
  | sed 's/.*:://' \
  | sort -u > "$tmp_metadata"

if ! diff -u "$tmp_variants" "$tmp_metadata" >/tmp/ui-parity-contract-diff.$$; then
  cat /tmp/ui-parity-contract-diff.$$ >&2
  rm -f /tmp/ui-parity-contract-diff.$$ || true
  fail "ParityException variants and PARITY_EXCEPTION_METADATA entries must stay in sync"
fi
rm -f /tmp/ui-parity-contract-diff.$$ || true

exception_count="$(rg -c 'exception: ParityException::' "$contract_file")"
reason_count="$(rg -c 'reason_code:' "$contract_file")"
scope_count="$(rg -c 'scope:' "$contract_file")"
surface_count="$(rg -c 'affected_surface:' "$contract_file")"
doc_count="$(rg -c 'doc_reference:' "$contract_file")"

[[ "$reason_count" == "$exception_count" ]] || fail "each parity exception metadata entry must declare reason_code"
[[ "$scope_count" == "$exception_count" ]] || fail "each parity exception metadata entry must declare scope"
[[ "$surface_count" == "$exception_count" ]] || fail "each parity exception metadata entry must declare affected_surface"
[[ "$doc_count" == "$exception_count" ]] || fail "each parity exception metadata entry must declare doc_reference"

rg -o 'doc_reference: "[^"]+"' "$contract_file" \
  | sed 's/^doc_reference: "//; s/"$//' > "$tmp_docs"
while IFS= read -r doc_path; do
  [[ -n "$doc_path" ]] || continue
  [[ -f "$doc_path" ]] || fail "parity exception doc reference does not exist: $doc_path"
done < "$tmp_docs"

cargo test -p aura-app shared_flow_support_contract_is_consistent --quiet
cargo test -p aura-app shared_flow_scenario_coverage_points_to_existing_scenarios --quiet
cargo test -p aura-app shared_screen_modal_and_list_support_is_unique_and_addressable --quiet
cargo test -p aura-app shared_screen_module_map_uses_canonical_screen_names --quiet
cargo test -p aura-app parity_module_map_points_to_existing_frontend_symbols --quiet
cargo test -p aura-app parity_exception_metadata_is_complete_and_documented --quiet

echo "ui parity contract: clean"
