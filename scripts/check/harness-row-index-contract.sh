#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-row-index-contract: $*" >&2
  exit 1
}

inventory="scenarios/harness_inventory.toml"
[[ -f "$inventory" ]] || fail "missing inventory: $inventory"

while IFS='|' read -r path class; do
  [[ -n "$path" ]] || continue
  [[ "$class" == "shared" ]] || continue
  [[ -f "$path" ]] || fail "missing shared scenario: $path"
  if rg -n '^\s*item_id\s*=\s*"(row[-_:]?[0-9]+|idx[-_:]?[0-9]+|index[-_:]?[0-9]+|[0-9]+)"\s*$' "$path" >/tmp/harness-row-index.$$ 2>/dev/null; then
    cat /tmp/harness-row-index.$$ >&2
    rm -f /tmp/harness-row-index.$$
    fail "shared scenario targets parity-critical list items by row index: $path"
  fi
done < <(
  awk '
    /^\[\[scenario\]\]/ { path=""; class="" }
    /^path = / { gsub(/^path = "|"$/, "", $0); path=$0; sub(/^path = /, "", path); gsub(/"/, "", path) }
    /^classification = / { class=$3; gsub(/"/, "", class) }
    /^notes = / { if (path != "" && class != "") print path "|" class }
  ' "$inventory"
)

cargo test -p aura-app snapshot_invariants_reject_row_index_ids --quiet
cargo test -p aura-app shared_intent_contract_rejects_row_index_item_ids --quiet

echo "harness row-index contract: clean"
