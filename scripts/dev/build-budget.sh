#!/usr/bin/env bash
# Budgeted Aura rebuild. Cargo's maxsize is a between-build soft target, not
# a real-time quota. This script never removes harness evidence or user data.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
root="$repo_root"
lane=manual
dry_run=0
allow_live_harness=0
no_prune=0
cap_gib="${AURA_BUILD_TARGET_CAP_GIB:-24}"
min_free_gib="${AURA_BUILD_MIN_FREE_GIB:-15}"
emergency_gib="${AURA_BUILD_EMERGENCY_FREE_GIB:-5}"
poll_seconds="${AURA_BUILD_POLL_SECONDS:-2}"

usage() {
  cat <<'EOF'
Usage: build-budget.sh [--root PATH] [--lane NAME] [--dry-run]
                       [--allow-live-harness] [--no-prune] -- COMMAND [ARGS...]

Defaults: target soft cap 24 GiB, admission floor 15 GiB free, emergency floor
5 GiB free. Override with AURA_BUILD_TARGET_CAP_GIB, AURA_BUILD_MIN_FREE_GIB,
AURA_BUILD_EMERGENCY_FREE_GIB (integer GiB). Run inside nix develop.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root) root="${2:?missing root}"; shift 2 ;;
    --lane) lane="${2:?missing lane}"; shift 2 ;;
    --dry-run) dry_run=1; shift ;;
    --allow-live-harness) allow_live_harness=1; shift ;;
    --no-prune) no_prune=1; shift ;;
    --) shift; break ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

root="$(cd "$root" && pwd -P)"
[[ -f "$root/Cargo.toml" ]] || { echo "build-budget: no Cargo.toml in $root" >&2; exit 2; }
[[ ! -L "$root/target" ]] || { echo 'build-budget: target is a symlink; refusing' >&2; exit 1; }
if [[ "$dry_run" -eq 0 && "$#" -eq 0 ]]; then
  echo 'build-budget: command required after --' >&2
  exit 2
fi
for value in "$cap_gib" "$min_free_gib" "$emergency_gib" "$poll_seconds"; do
  [[ "$value" =~ ^[0-9]+$ ]] || { echo 'build-budget: budgets and poll interval must be whole numbers' >&2; exit 2; }
done
(( cap_gib > 0 && min_free_gib > emergency_gib && emergency_gib > 0 && poll_seconds > 0 )) || {
  echo 'build-budget: invalid budget ordering' >&2
  exit 2
}

free_kib() { df -Pk "$root" | awk 'NR == 2 {print $4}'; }
target_kib() {
  if [[ -d "$root/target" ]]; then du -sk "$root/target" | awk 'NR == 1 {print $1}';
  else printf '0\n'; fi
}
cap_kib=$((cap_gib * 1024 * 1024))
min_free_kib=$((min_free_gib * 1024 * 1024))
emergency_kib=$((emergency_gib * 1024 * 1024))

active_consumers() {
  ps -axo pid=,comm= | awk -v own="$$" -v allow="$allow_live_harness" '
    {
      pid=$1; name=$2; sub(/^.*\//, "", name);
      builder = (name ~ /^(cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep)$/);
      harness = (name ~ /^(tool_repl|aura-harness|aura)$/);
      if (pid != own && (builder || (!allow && harness)))
        printf "%s(%s) ", name, pid
    }'
}
require_idle() {
  local found
  found="$(active_consumers)"
  if [[ -n "$found" ]]; then
    echo "build-budget: refusing to sweep/build while another builder or harness consumer is active: $found" >&2
    return 1
  fi
}
target_has_open_files() {
  local open_files
  if ! command -v lsof >/dev/null; then
    echo 'build-budget: lsof unavailable; treating target as in use' >&2
    return 0
  fi
  if ! open_files="$(lsof -n -P 2>/dev/null)"; then
    echo 'build-budget: open-file scan failed; treating target as in use' >&2
    return 0
  fi
  rg -F -q "$root/target/" <<< "$open_files"
}
sweep() {
  local mode="$1" preview release_count rc
  preview="$(mktemp "${TMPDIR:-/tmp}/aura-sweep-preview.XXXXXX")"
  printf 'Sweep preview: cargo sweep --maxsize %sGiB --dry-run -v .\n' "$cap_gib"
  if (cd "$root" && cargo sweep --maxsize "${cap_gib}GiB" --dry-run -v .) > "$preview" 2>&1; then
    rg '\[INFO\]' "$preview" || cat "$preview"
  else
    rc=$?
    cat "$preview" >&2
    rm -f "$preview"
    return "$rc"
  fi
  release_count="$(rg -c 'Would remove: .*target/(release/|wasm-release/|wasm32-unknown-unknown/release/)' "$preview" || true)"
  release_count="${release_count:-0}"
  printf 'Protected production/web candidates: %s\n' "$release_count"
  rm -f "$preview"
  [[ "$mode" == dry ]] && return 0
  if (( release_count > 0 || allow_live_harness == 1 )); then
    echo 'build-budget: preserving production/web caches and live harness artifacts; skipping global sweep' >&2
    return 3
  fi
  require_idle || return $?
  if target_has_open_files; then
    echo 'build-budget: target has open files; skipping global sweep' >&2
    return 3
  fi
  printf 'Sweep: cargo sweep --maxsize %sGiB .\n' "$cap_gib"
  (cd "$root" && cargo sweep --maxsize "${cap_gib}GiB" .)
}
prune_safe_lanes() {
  local mode="$1" candidate
  local candidates=(wasm-debug dylint debug)
  if (( allow_live_harness == 1 )); then candidates=(wasm-debug); fi
  for candidate in "${candidates[@]}"; do
    if [[ "$mode" == dry ]]; then
      bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$root" --lane "$candidate" --dry-run
    else
      require_idle || return $?
      if ! bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$root" \
        --lane "$candidate" --apply --lock-owned-by "$$"; then
        echo "build-budget: $candidate remains protected or busy" >&2
        require_idle || return $?
      fi
      if (( $(target_kib) <= cap_kib && $(free_kib) >= min_free_kib )); then
        break
      fi
    fi
  done
}

printf 'Budget: target=%s GiB soft; admission=%s GiB free; emergency=%s GiB free\n' \
  "$cap_gib" "$min_free_gib" "$emergency_gib"
before_free="$(free_kib)"
before_target="$(target_kib)"
printf 'Before: free=%s KiB target=%s KiB\n' "$before_free" "$before_target"
if [[ "$dry_run" -eq 1 ]]; then
  if (( no_prune == 1 )); then
    echo 'No-prune mode: cache collection disabled; admission and emergency floors remain active'
  else
    sweep dry
    prune_safe_lanes dry
  fi
  echo 'Dry run: no build or deletion performed'
  exit 0
fi

lock_dir="$root/target/.aura-build-budget.lock"
mkdir -p "$root/target"
if ! mkdir "$lock_dir" 2>/dev/null; then
  echo "build-budget: another budgeted build holds $lock_dir (or a stale lock needs review)" >&2
  exit 1
fi
printf '%s\n' "$$" > "$lock_dir/pid"
child_pid=''
stop_owned_group() {
  local pid="$1" attempt
  [[ -n "$pid" ]] || return 0
  kill -TERM -- "-$pid" 2>/dev/null || true
  for attempt in 1 2 3; do
    kill -0 -- "-$pid" 2>/dev/null || return 0
    sleep 1
  done
  kill -KILL -- "-$pid" 2>/dev/null || true
}
cleanup() {
  if [[ -n "$child_pid" ]] && kill -0 "$child_pid" 2>/dev/null; then
    stop_owned_group "$child_pid"
    wait "$child_pid" 2>/dev/null || true
  fi
  rm -f "$lock_dir/pid"
  rmdir "$lock_dir" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

require_idle
if (( before_target > cap_kib || before_free < min_free_kib )); then
  if (( no_prune == 1 )); then
    echo 'build-budget: no-prune mode cannot recover the required headroom or target cap' >&2
    exit 1
  fi
  require_idle
  if sweep apply; then :;
  else
    sweep_status=$?
    if (( sweep_status == 3 )); then prune_safe_lanes apply;
    else exit "$sweep_status"; fi
  fi
fi
pre_free="$(free_kib)"
pre_target="$(target_kib)"
printf 'Pre-build: free=%s KiB target=%s KiB\n' "$pre_free" "$pre_target"
if (( pre_free < min_free_kib )); then
  echo "build-budget: insufficient headroom: need ${min_free_gib} GiB free before building" >&2
  exit 1
fi

started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
profile="${AURA_BUILD_PROFILE:-unlabelled}"
features="${AURA_BUILD_FEATURES:-unlabelled}"
target_triple="${AURA_BUILD_TARGET_TRIPLE:-$(rustc -vV 2>/dev/null | awk '/^host: / {print $2}' || true)}"
printf 'Build lane=%s command:' "$lane"
printf ' %q' "$@"
printf '\n'
set -m # Give the background build its own process group for bounded shutdown.
(cd "$root" && "$@") &
child_pid=$!
set +m
lowest_free="$pre_free"
peak_target="$pre_target"
poll_count=0
status=0
while kill -0 "$child_pid" 2>/dev/null; do
  sleep "$poll_seconds"
  poll_count=$((poll_count + 1))
  now_free="$(free_kib)"
  (( now_free < lowest_free )) && lowest_free="$now_free"
  if (( poll_count % 5 == 0 )); then
    now_target="$(target_kib)"
    (( now_target > peak_target )) && peak_target="$now_target"
  fi
  if (( now_free < emergency_kib )); then
    echo "build-budget: emergency floor reached; stopping only this build" >&2
    stop_owned_group "$child_pid"
    status=75
    break
  fi
done
if wait "$child_pid"; then :; else child_status=$?; (( status == 0 )) && status="$child_status"; fi
stop_owned_group "$child_pid" # Reap any build-owned background descendants.
child_pid=''
post_build_target="$(target_kib)"
(( post_build_target > peak_target )) && peak_target="$post_build_target"

if (( status == 0 && no_prune == 1 )); then
  echo 'No-prune mode: post-build cache collection skipped'
elif (( status == 0 )); then
  if require_idle; then
    if sweep apply; then :;
    else
      sweep_status=$?
      if (( sweep_status == 3 )); then
        prune_safe_lanes apply
      else
        echo 'build-budget: post-build sweep failed' >&2
        status=76
      fi
    fi
  else
    echo 'build-budget: post-build sweep skipped because another builder started' >&2
    status=76
  fi
fi
after_free="$(free_kib)"
after_target="$(target_kib)"
printf 'After: free=%s KiB target=%s KiB lowest-free=%s KiB sampled-peak-target=%s KiB exit=%s\n' \
  "$after_free" "$after_target" "$lowest_free" "$peak_target" "$status"
mkdir -p "$root/artifacts/disk-budget"
log_path="$root/artifacts/disk-budget/builds.tsv"
if [[ ! -e "$log_path" ]]; then
  printf 'started_utc\tfinished_utc\tlane\tprofile\ttarget_triple\tfeatures\tcap_gib\tmin_free_gib\temergency_gib\tbefore_free_kib\tbefore_target_kib\tpre_free_kib\tpre_target_kib\tlowest_free_kib\tpeak_target_kib\tafter_free_kib\tafter_target_kib\texit_status\n' > "$log_path"
fi
if (( status == 0 )); then
  report_path="$(mktemp "$root/artifacts/disk-budget/report.XXXXXX")"
  if ! AURA_BUILD_LANE="$lane" AURA_BUILD_PROFILE="$profile" \
    AURA_BUILD_TARGET_TRIPLE="$target_triple" AURA_BUILD_FEATURES="$features" \
    bash "$repo_root/scripts/dev/disk-report.sh" "$root" | tee "$report_path"; then
    echo "build-budget: disk report failed; partial report at $report_path" >&2
    status=77
  else
    printf 'Saved disk report: %s\n' "$report_path"
  fi
fi
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
  "$started" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$lane" "$profile" \
  "$target_triple" "$features" "$cap_gib" \
  "$min_free_gib" "$emergency_gib" "$before_free" "$before_target" \
  "$pre_free" "$pre_target" "$lowest_free" "$peak_target" "$after_free" "$after_target" "$status" >> "$log_path"
exit "$status"
