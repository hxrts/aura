#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

inventory="scenarios/harness_inventory.toml"
matrix_runner="scripts/harness/run-matrix.sh"

fail() {
  echo "harness-matrix-inventory: $*" >&2
  exit 1
}

[[ -f "$inventory" ]] || fail "missing inventory: $inventory"
[[ -x "$matrix_runner" ]] || fail "missing matrix runner: $matrix_runner"

collect_expected_ids() {
  local lane="$1"
  awk -v lane="$lane" '
    BEGIN { id=""; class=""; status="" }
    /^\[\[scenario\]\]/ {
      if (id != "") emit()
      id=""; class=""; status=""
      next
    }
    /^id = / {
      gsub(/^id = |"/, "", $0)
      id=$0
      next
    }
    /^classification = / {
      gsub(/^classification = |"/, "", $0)
      class=$0
      next
    }
    /^migration_status = / {
      gsub(/^migration_status = |"/, "", $0)
      status=$0
      next
    }
    function emit() {
      if (status != "converted") return
      if (lane == "tui" && (class == "shared" || class == "tui_conformance")) print id
      if (lane == "web" && (class == "shared" || class == "web_conformance")) print id
    }
    END {
      if (id != "") emit()
    }
  ' "$inventory" | sort -u
}

collect_actual_ids() {
  local lane="$1"
  bash "$matrix_runner" --lane "$lane" --dry-run \
    | rg -o 'scenario=[A-Za-z0-9._-]+' \
    | sed 's/^scenario=//' \
    | sort -u
}

compare_lane() {
  local lane="$1"
  local expected_file actual_file
  expected_file="$(mktemp)"
  actual_file="$(mktemp)"
  trap 'rm -f "$expected_file" "$actual_file"' RETURN

  collect_expected_ids "$lane" > "$expected_file"
  collect_actual_ids "$lane" > "$actual_file"

  if ! diff -u "$expected_file" "$actual_file" >/tmp/harness-matrix-inventory-diff.$$; then
    cat /tmp/harness-matrix-inventory-diff.$$ >&2
    rm -f /tmp/harness-matrix-inventory-diff.$$ || true
    fail "lane $lane does not match inventory-derived converted scenario set"
  fi
  rm -f /tmp/harness-matrix-inventory-diff.$$ || true

  echo "• matrix inventory OK for lane=$lane"
}

compare_lane tui
compare_lane web

echo "harness matrix inventory: clean"
