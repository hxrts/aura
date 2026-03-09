#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
inventory="$repo_root/scenarios/harness_inventory.toml"

fail() {
  echo "run-matrix: $*" >&2
  exit 1
}

lane=""
dry_run=0
scenario_ids=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --lane)
      lane="${2:-}"
      shift 2
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    --scenario-id)
      scenario_ids+=("${2:-}")
      shift 2
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

[[ -n "$lane" ]] || fail "--lane is required"
[[ "$lane" == "tui" || "$lane" == "web" || "$lane" == "all" ]] || fail "invalid lane: $lane"
[[ -f "$inventory" ]] || fail "missing inventory: $inventory"

lane_match() {
  local scenario_class="$1"
  local selected_lane="$2"
  case "$selected_lane" in
    tui)
      [[ "$scenario_class" == "shared" || "$scenario_class" == "tui_only" ]]
      ;;
    web)
      [[ "$scenario_class" == "shared" || "$scenario_class" == "web_only" ]]
      ;;
    *)
      return 1
      ;;
  esac
}

config_for_class() {
  case "$1" in
    shared) echo "configs/harness/mixed-web-tui-loopback.toml" ;;
    tui_only) echo "configs/harness/local-loopback.toml" ;;
    web_only) echo "configs/harness/browser-loopback.toml" ;;
    *) return 1 ;;
  esac
}

scenario_id_requested() {
  local scenario_id="$1"
  [[ ${#scenario_ids[@]} -eq 0 ]] && return 0
  local requested
  for requested in "${scenario_ids[@]}"; do
    [[ "$requested" == "$scenario_id" ]] && return 0
  done
  return 1
}

run_lane() {
  local selected_lane="$1"
  local scenario_id=""
  local scenario_path=""
  local scenario_class=""
  local migration_status=""
  local config=""
  local count=0

  echo "[harness-matrix] lane=$selected_lane begin"

  while IFS='|' read -r scenario_id scenario_path scenario_class migration_status; do
    [[ -n "$scenario_id" ]] || continue
    lane_match "$scenario_class" "$selected_lane" || continue
    [[ "$migration_status" == "converted" ]] || continue
    scenario_id_requested "$scenario_id" || continue

    config="$(config_for_class "$scenario_class")" || fail "no config for classification: $scenario_class"
    count=$((count + 1))

    echo "[harness-matrix] lane=$selected_lane scenario=$scenario_id class=$scenario_class config=$config"
    if [[ $dry_run -eq 0 ]]; then
      (
        cd "$repo_root"
        cargo run -q -p aura-harness --bin aura-harness -- run --config "$config" --scenario "$scenario_path"
      )
    fi
  done < <(
    awk '
      BEGIN { id=""; path=""; class=""; status="" }
      /^\[\[scenario\]\]/ {
        if (id != "") print id "|" path "|" class "|" status
        id=""; path=""; class=""; status=""
      }
      /^id = / {
        gsub(/^id = |"/, "", $0)
        id=$0
      }
      /^path = / {
        gsub(/^path = |"/, "", $0)
        path=$0
      }
      /^classification = / {
        gsub(/^classification = |"/, "", $0)
        class=$0
      }
      /^migration_status = / {
        gsub(/^migration_status = |"/, "", $0)
        status=$0
      }
      END {
        if (id != "") print id "|" path "|" class "|" status
      }
    ' "$inventory"
  )

  echo "[harness-matrix] lane=$selected_lane scenarios=$count done"
}

if [[ "$lane" == "all" ]]; then
  run_lane tui
  run_lane web
else
  run_lane "$lane"
fi
