#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
inventory="$repo_root/scenarios/harness_inventory.toml"

fail() {
  echo "run-matrix: $*" >&2
  exit 1
}

lane=""
suite="all"
dry_run=0
scenario_ids=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --lane)
      lane="${2:-}"
      shift 2
      ;;
    --suite)
      suite="${2:-}"
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
[[ "$suite" == "shared" || "$suite" == "conformance" || "$suite" == "all" ]] || fail "invalid suite: $suite"
[[ -f "$inventory" ]] || fail "missing inventory: $inventory"

suite_match() {
  local scenario_class="$1"
  case "$suite" in
    shared)
      [[ "$scenario_class" == "shared" ]]
      ;;
    conformance)
      [[ "$scenario_class" == "tui_conformance" || "$scenario_class" == "web_conformance" ]]
      ;;
    all)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

lane_match() {
  local scenario_class="$1"
  local selected_lane="$2"
  case "$selected_lane" in
    tui)
      [[ "$scenario_class" == "shared" || "$scenario_class" == "tui_conformance" ]]
      ;;
    web)
      [[ "$scenario_class" == "shared" || "$scenario_class" == "web_conformance" ]]
      ;;
    *)
      return 1
      ;;
  esac
}

config_for_run() {
  local scenario_class="$1"
  local selected_lane="$2"
  case "$selected_lane:$scenario_class" in
    tui:shared|tui:tui_conformance) echo "configs/harness/local-loopback.toml" ;;
    web:shared|web:web_conformance) echo "configs/harness/browser-loopback.toml" ;;
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

run_scenario() {
  local config="$1"
  local scenario_path="$2"
  if [[ -n "${AURA_HARNESS_BIN:-}" ]]; then
    "$AURA_HARNESS_BIN" run --config "$config" --scenario "$scenario_path"
  else
    cargo run -q -p aura-harness --bin aura-harness -- run --config "$config" --scenario "$scenario_path"
  fi
}

clean_config_state() {
  local config="$1"
  local path=""

  while IFS= read -r path; do
    [[ -n "$path" ]] || continue
    rm -rf "$repo_root/$path"
  done < <(
    awk '
      /^artifact_dir = / || /^data_dir = / {
        gsub(/^[^=]+ = |"/, "", $0)
        print $0
      }
    ' "$config"
  )
}

run_token_for_scenario() {
  local selected_lane="$1"
  local scenario_id="$2"
  printf '%s-%s-%s-%s' \
    "$selected_lane" \
    "$scenario_id" \
    "$$" \
    "$(date +%s)"
}

run_lane() {
  local selected_lane="$1"
  local scenario_id=""
  local scenario_path=""
  local scenario_class=""
  local config=""
  local count=0

  echo "[harness-matrix] lane=$selected_lane suite=$suite begin"

  while IFS='|' read -r scenario_id scenario_path scenario_class; do
    [[ -n "$scenario_id" ]] || continue
    suite_match "$scenario_class" || continue
    lane_match "$scenario_class" "$selected_lane" || continue
    scenario_id_requested "$scenario_id" || continue

    config="$(config_for_run "$scenario_class" "$selected_lane")" || fail "no config for classification $scenario_class on lane $selected_lane"
    count=$((count + 1))

    echo "[harness-matrix] lane=$selected_lane suite=$suite scenario=$scenario_id class=$scenario_class config=$config"
    if [[ $dry_run -eq 0 ]]; then
      local run_token=""
      run_token="$(run_token_for_scenario "$selected_lane" "$scenario_id")"
      (
        cd "$repo_root"
        export AURA_HARNESS_RUN_TOKEN="$run_token"
        run_scenario "$config" "$scenario_path"
      )
    fi
  done < <(
    awk '
      BEGIN { id=""; path=""; class="" }
      /^\[\[scenario\]\]/ {
        if (id != "") print id "|" path "|" class
        id=""; path=""; class=""
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
      END {
        if (id != "") print id "|" path "|" class
      }
    ' "$inventory"
  )

  echo "[harness-matrix] lane=$selected_lane suite=$suite scenarios=$count done"
}

if [[ "$lane" == "all" ]]; then
  run_lane tui
  run_lane web
else
  run_lane "$lane"
fi
