#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "harness-scenario-inventory: $*" >&2
  exit 1
}

inventory="scenarios/harness_inventory.toml"
[[ -f "$inventory" ]] || fail "missing inventory: $inventory"

mapfile -t scenario_files < <(find scenarios/harness -maxdepth 1 -name '*.toml' | sort)
mapfile -t inventory_paths < <(rg '^path\s*=\s*"([^"]+)"' -or '$1' "$inventory" | sort)
mapfile -t inventory_ids < <(rg '^id\s*=\s*"([^"]+)"' -or '$1' "$inventory" | sort)
mapfile -t inventory_classes < <(rg '^classification\s*=\s*"([^"]+)"' -or '$1' "$inventory" | sort)
mapfile -t inventory_statuses < <(rg '^migration_status\s*=\s*"([^"]+)"' -or '$1' "$inventory" | sort)

[[ ${#scenario_files[@]} -eq ${#inventory_paths[@]} ]] || fail "inventory path count (${#inventory_paths[@]}) does not match scenario file count (${#scenario_files[@]})"

for path in "${scenario_files[@]}"; do
  printf '%s\n' "${inventory_paths[@]}" | rg -qx "$path" || fail "scenario missing from inventory: $path"
done
for path in "${inventory_paths[@]}"; do
  [[ -f "$path" ]] || fail "inventory references missing scenario: $path"
done

for class in shared web_only tui_only to_be_removed; do
  printf '%s\n' "${inventory_classes[@]}" | rg -qx "$class" >/dev/null || true
done
if printf '%s\n' "${inventory_classes[@]}" | rg -vx '(shared|web_only|tui_only|to_be_removed)' >/tmp/harness-inventory-bad-class.$$ 2>/dev/null; then
  cat /tmp/harness-inventory-bad-class.$$ >&2
  rm -f /tmp/harness-inventory-bad-class.$$
  fail "inventory contains invalid classification"
fi
rm -f /tmp/harness-inventory-bad-class.$$ || true

if printf '%s\n' "${inventory_statuses[@]}" | rg -vx '(converted|legacy_pending_conversion|superseded)' >/tmp/harness-inventory-bad-status.$$ 2>/dev/null; then
  cat /tmp/harness-inventory-bad-status.$$ >&2
  rm -f /tmp/harness-inventory-bad-status.$$
  fail "inventory contains invalid migration_status"
fi
rm -f /tmp/harness-inventory-bad-status.$$ || true

if [[ $(printf '%s\n' "${inventory_ids[@]}" | uniq -d | wc -l | tr -d ' ') != "0" ]]; then
  fail "inventory contains duplicate scenario ids"
fi
if [[ $(printf '%s\n' "${inventory_paths[@]}" | uniq -d | wc -l | tr -d ' ') != "0" ]]; then
  fail "inventory contains duplicate scenario paths"
fi

echo "harness scenario inventory: clean"
