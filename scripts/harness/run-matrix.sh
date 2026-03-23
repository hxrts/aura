#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
inventory="$repo_root/scenarios/harness_inventory.toml"

fail() {
  echo "run-matrix: $*" >&2
  exit 1
}

cleanup_run_scope() {
  local run_root="${1:-}"
  local transient_root="${2:-}"
  local harness_pid="${3:-}"

  kill_process_tree() {
    local pid="${1:-}"
    [[ -n "$pid" ]] || return 0
    local child_pid=""
    while IFS= read -r child_pid; do
      [[ -n "$child_pid" ]] || continue
      kill_process_tree "$child_pid"
    done < <(pgrep -P "$pid" 2>/dev/null || true)
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  }

  kill_process_tree "$harness_pid"

  cleanup_root_tree() {
    local root="${1:-}"
    [[ -n "$root" && -d "$root" ]] || return 0
    while IFS= read -r pid_file; do
      [[ -n "$pid_file" ]] || continue
      if [[ -f "$pid_file" ]]; then
        local child_pid=""
        child_pid="$(tr -d '[:space:]' < "$pid_file" 2>/dev/null || true)"
        kill_process_tree "$child_pid"
      fi
    done < <(find "$root" -type f -name '*.pid' 2>/dev/null)
    find "$root" -type s -delete 2>/dev/null || true
    find "$root" -type f \( -name '*.pid' -o -name 'clipboard.txt' -o -name '.bootstrap-runtime-handoff-ready' \) -delete 2>/dev/null || true
    rm -rf "$root" 2>/dev/null || true
  }

  cleanup_root_tree "$transient_root"
  cleanup_root_tree "$run_root"
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

ensure_built_artifact() {
  local package="$1"
  local bin_name="$2"
  local artifact_path="$repo_root/target/debug/$bin_name"

  echo "[harness-matrix] building package=$package bin=$bin_name" >&2
  (
    cd "$repo_root"
    cargo build -q -p "$package" --bin "$bin_name"
  )
  [[ -x "$artifact_path" ]] || fail "missing built artifact: $artifact_path"
  printf '%s\n' "$artifact_path"
}

prepare_lane_artifacts() {
  local selected_lane="$1"

  if [[ -z "${AURA_HARNESS_BIN:-}" ]]; then
    export AURA_HARNESS_BIN
    AURA_HARNESS_BIN="$(ensure_built_artifact aura-harness aura-harness)"
  fi

  case "$selected_lane" in
    tui)
      if [[ -z "${AURA_HARNESS_AURA_BIN:-}" ]]; then
        export AURA_HARNESS_AURA_BIN
        AURA_HARNESS_AURA_BIN="$(ensure_built_artifact aura-terminal aura)"
      fi
      ;;
    web)
      ;;
    *)
      fail "unknown lane for artifact preparation: $selected_lane"
      ;;
  esac
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

  if [[ $dry_run -eq 0 ]]; then
    prepare_lane_artifacts "$selected_lane"
  fi

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
      local run_root=""
      local transient_root=""
      local transient_key=""
      local harness_pid=""
      run_token="$(run_token_for_scenario "$selected_lane" "$scenario_id")"
      run_root="$repo_root/.tmp/harness/matrix/$selected_lane/$scenario_id/$run_token"
      transient_key="$(printf '%s' "$run_token" | cksum | awk '{print $1}')"
      transient_root="$repo_root/.tmp/harness/transient/$transient_key"
      (
        cd "$repo_root"
        export AURA_HARNESS_RUN_TOKEN="$run_token"
        export AURA_HARNESS_RUN_ROOT="$run_root"
        export AURA_HARNESS_TRANSIENT_ROOT="$transient_root"
        trap 'cleanup_run_scope "$AURA_HARNESS_RUN_ROOT" "$AURA_HARNESS_TRANSIENT_ROOT" "${harness_pid:-}"' EXIT INT TERM
        mkdir -p "$AURA_HARNESS_RUN_ROOT"
        mkdir -p "$AURA_HARNESS_TRANSIENT_ROOT"
        run_scenario "$config" "$scenario_path" &
        harness_pid=$!
        wait "$harness_pid"
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
