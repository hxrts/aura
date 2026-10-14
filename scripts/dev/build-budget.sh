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
preserve_cache_lane=''
cap_gib="${AURA_BUILD_TARGET_CAP_GIB:-10}"
min_free_gib="${AURA_BUILD_MIN_FREE_GIB:-15}"
emergency_gib="${AURA_BUILD_EMERGENCY_FREE_GIB:-5}"
poll_seconds="${AURA_BUILD_POLL_SECONDS:-2}"
reserve_gib="${AURA_BUILD_RESERVE_GIB:-4}"
shared_dir="${AURA_BUILD_SHARED_DIR:-$HOME/.cache/aura-build}"
# Debug caches accumulate across rebuilds. Keep the measured bounded default,
# with an explicit opt-in for workloads that benefit from incremental reuse.
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
[[ "$CARGO_INCREMENTAL" == 0 || "$CARGO_INCREMENTAL" == 1 ]] || {
  echo 'build-budget: CARGO_INCREMENTAL must be 0 or 1' >&2; exit 2;
}

usage() {
  cat <<'EOF'
Usage: build-budget.sh [--root PATH] [--lane NAME] [--dry-run]
                       [--preserve-cache-lane wasm-debug]
                       [--allow-live-harness] [--no-prune] -- COMMAND [ARGS...]

Defaults: per-checkout target soft cap 10 GiB, volume admission floor 15 GiB free
(after other admitted builds' and incoming measured-growth reservations), emergency floor
5 GiB free. Override with AURA_BUILD_TARGET_CAP_GIB, AURA_BUILD_MIN_FREE_GIB,
AURA_BUILD_EMERGENCY_FREE_GIB, AURA_BUILD_RESERVE_GIB (fallback integer GiB) and
AURA_BUILD_SHARED_DIR (default ~/.cache/aura-build). Run inside nix develop.
Cargo output always goes to this checkout's target/ (CARGO_TARGET_DIR is unset).
Incremental compilation defaults to 0; set CARGO_INCREMENTAL=1 to opt in.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root) root="${2:?missing root}"; shift 2 ;;
    --lane) lane="${2:?missing lane}"; shift 2 ;;
    --dry-run) dry_run=1; shift ;;
    --allow-live-harness) allow_live_harness=1; shift ;;
    --no-prune) no_prune=1; shift ;;
    --preserve-cache-lane) preserve_cache_lane="${2:?missing cache lane}"; shift 2 ;;
    --) shift; break ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done

# Direct Cargo wasm test/check/build commands identify their own cache. Shell
# batches must declare the incoming lane explicitly because their argv hides
# the actual compiler command from this wrapper.
if [[ -z "$preserve_cache_lane" && "${1:-}" == cargo ]]; then
  cargo_target='' cargo_profile=debug
  cargo_args=("$@")
  for ((index=1; index<${#cargo_args[@]}; index++)); do
    case "${cargo_args[index]}" in
      --target)
        if (( index + 1 < ${#cargo_args[@]} )); then cargo_target="${cargo_args[index+1]}"; fi ;;
      --target=*) cargo_target="${cargo_args[index]#--target=}" ;;
      --release|--profile) cargo_profile=other ;;
      --profile=*) cargo_profile=other ;;
    esac
  done
  if [[ "$cargo_target" == wasm32-unknown-unknown && "$cargo_profile" == debug ]]; then
    preserve_cache_lane=wasm-debug
  fi
fi

case "$preserve_cache_lane" in
  ''|wasm-debug) ;;
  *) echo 'build-budget: unsupported preserve-cache-lane' >&2; exit 2 ;;
esac

root="$(cd "$root" && pwd -P)"
[[ -f "$root/Cargo.toml" ]] || { echo "build-budget: no Cargo.toml in $root" >&2; exit 2; }
# Cargo writes to this checkout's target/ even when the shell exported
# another checkout's CARGO_TARGET_DIR. Unset rather than overridden: sccache
# hashes CARGO_* variables, and a per-checkout value would defeat sharing.
unset CARGO_TARGET_DIR
[[ ! -L "$root/target" ]] || { echo 'build-budget: target is a symlink; refusing' >&2; exit 1; }
if [[ "$dry_run" -eq 0 && "$#" -eq 0 ]]; then
  echo 'build-budget: command required after --' >&2
  exit 2
fi
for value in "$cap_gib" "$min_free_gib" "$emergency_gib" "$poll_seconds" "$reserve_gib"; do
  [[ "$value" =~ ^[0-9]+$ ]] || { echo 'build-budget: budgets and poll interval must be whole numbers' >&2; exit 2; }
done
(( cap_gib > 0 && min_free_gib > emergency_gib && emergency_gib > 0 && poll_seconds > 0 )) || {
  echo 'build-budget: invalid budget ordering' >&2
  exit 2
}
# A gate recipe runs under one budgeted invocation that already holds this
# checkout's lock and admission and sweeps afterwards; nested invocations from
# inside it (e.g. the ownership gate running web-check) run their command
# directly instead of waiting on their own parent.
if [[ "$dry_run" -eq 0 && "${AURA_BUILD_BUDGET_HELD:-}" == "$root" ]]; then
  echo "build-budget: lane $lane runs inside the held budget for $root"
  cd "$root" && exec "$@"
fi

free_kib() { df -Pk "$root" | awk 'NR == 2 {print $4}'; }
target_kib() {
  local sampled mode="${1:-best-effort}"
  if [[ ! -d "$root/target" ]]; then printf '0\n'; return; fi
  # Cargo can unlink a temporary artifact while du is walking target/. A
  # partial size sample is still useful; losing it must not stop the build.
  sampled="$(du -sk "$root/target" 2>/dev/null | awk 'NR == 1 {print $1}' || true)"
  if [[ "$sampled" =~ ^[0-9]+$ ]]; then printf '%s\n' "$sampled";
  elif [[ "$mode" == strict ]]; then
    echo 'build-budget: could not measure target size before build' >&2
    return 1
  else printf '0\n'; fi
}
cap_kib=$((cap_gib * 1024 * 1024))
min_free_kib=$((min_free_gib * 1024 * 1024))
emergency_kib=$((emergency_gib * 1024 * 1024))

# Only this checkout's builders and harness consumers block its sweeps;
# sibling worktrees build concurrently against their own target/.
source "$repo_root/scripts/dev/scoped-builders.sh"
source "$repo_root/scripts/dev/cache-rank.sh"
active_consumers() {
  local names='cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep'
  (( allow_live_harness == 1 )) || names="$names|tool_repl|aura-harness|aura"
  aura_scoped_processes "$root" "$names" "$$"
}

# Volume-wide admission shared by every checkout on this machine. Each
# admitted build holds a reservation of its expected growth until it exits; a
# new build is admitted only if free space minus both its measured incoming
# growth and other live reservations stays above the floor. The admission lock is held only while checking and
# reserving, never during the build.
reservation=''
incoming_reservation_kib=$((reserve_gib * 1024 * 1024))
estimate_incoming_growth() {
  local receipt measured largest=-1 legacy_log="$root/artifacts/disk-budget/builds.tsv"
  # Receipts are written under this checkout and identify the workload lane.
  # A failed command can still leave artifacts, so its observed peak counts.
  for receipt in "$root"/artifacts/disk-budget/receipts/run.*/receipt.json; do
    [[ -f "$receipt" && ! -L "$receipt" ]] || continue
    measured="$(jq -er --arg root "$root" --arg lane "$lane" \
      'select(.root==$root and .lane==$lane and
        (.command_exit_status!=null or .child.pid!=null)) |
       .disk.measured_growth_kib | select(type=="number" and .>=0) | floor' \
      "$receipt" 2>/dev/null)" || continue
    (( measured > largest )) && largest="$measured"
  done
  # Older runs recorded the same peaks only in builds.tsv. A same-lane command
  # that failed before linking measured only its partial growth, so it cannot
  # displace a larger recent profile/target peak from a completed run.
  if [[ -f "$legacy_log" && ! -L "$legacy_log" ]]; then
    measured="$(awk -F '\t' -v profile="$profile" -v triple="$target_triple" '
      NR>1 && $4==profile && $5==triple && $12~/^[0-9]+$/ && $13~/^[0-9]+$/ &&
      $14~/^[0-9]+$/ && $15~/^[0-9]+$/ {
        free_growth=$12-$14; target_growth=$15-$13
        growth=free_growth>target_growth?free_growth:target_growth
        if (growth<0) growth=0
        print growth
      }' "$legacy_log" | tail -8 | sort -nr | head -1)"
    if [[ "$measured" =~ ^[0-9]+$ ]] && (( measured > largest )); then
      largest="$measured"
    fi
  fi
  if (( largest >= 0 )); then
    # One GiB covers sparse peak sampling and gives even a zero-growth command
    # a useful reservation; the 4 GiB setting remains the cold-start fallback.
    incoming_reservation_kib=$((largest + 1024 * 1024))
  fi
}
other_reservations_kib() {
  local file pid total=0 kib
  for file in "$shared_dir"/reservations/*; do
    [[ -f "$file" ]] || continue
    pid="${file##*/}"
    if [[ ! "$pid" =~ ^[0-9]+$ ]] || ! kill -0 "$pid" 2>/dev/null; then
      rm -f "$file"; continue
    fi
    [[ "$pid" == "$$" ]] && continue
    kib="$(awk 'NR == 1 {print $1}' "$file")"
    [[ "$kib" =~ ^[0-9]+$ ]] && total=$((total + kib))
  done
  printf '%s\n' "$total"
}
admit_build() {
  local lock="$shared_dir/admission.lock" attempt holder free others
  mkdir -p "$shared_dir/reservations"
  for attempt in $(seq 1 50); do
    if mkdir "$lock" 2>/dev/null; then break; fi
    holder="$(cat "$lock/pid" 2>/dev/null || true)"
    if [[ "$holder" =~ ^[0-9]+$ ]] && ! kill -0 "$holder" 2>/dev/null; then
      rm -f "$lock/pid"; rmdir "$lock" 2>/dev/null || true
    fi
    sleep 0.2
  done
  [[ -d "$lock" ]] || { echo "build-budget: admission lock busy: $lock" >&2; return 1; }
  printf '%s\n' "$$" > "$lock/pid"
  free="$(free_kib)"
  others="$(other_reservations_kib)"
  printf 'Admission: free=%s KiB other-reservations=%s KiB incoming-reservation=%s KiB floor=%s GiB\n' \
    "$free" "$others" "$incoming_reservation_kib" "$min_free_gib"
  if (( free - others - incoming_reservation_kib < min_free_kib )); then
    aura_receipt_update --arg reason insufficient-headroom \
      --argjson free "$free" --argjson others "$others" \
      --argjson incoming "$incoming_reservation_kib" \
      --argjson floor "$min_free_kib" \
      '.admission.admitted=false | .admission.refusal={reason:$reason,
        free_kib:$free,other_reservations_kib:$others,
        incoming_reservation_kib:$incoming,floor_kib:$floor} |
        .admission.refusal_count=((.admission.refusal_count // 0)+1)'
    rm -f "$lock/pid"; rmdir "$lock"
    echo "build-budget: insufficient headroom: need ${min_free_gib} GiB free after other admitted builds' reservations and incoming measured growth" >&2
    return 1
  fi
  reservation="$shared_dir/reservations/$$"
  printf '%s\n' "$incoming_reservation_kib" > "$reservation"
  aura_receipt_update --argjson free "$free" --argjson others "$others" \
    '.admission.admitted=true | .admission.admitted_free_kib=$free |
      .admission.admitted_other_reservations_kib=$others'
  rm -f "$lock/pid"; rmdir "$lock"
}
idle_now() {
  busy="$(active_consumers)"
  [[ -z "$busy" ]]
}
# Wait (AURA_BUILD_WAIT_SECONDS, shared with the lock and admission waits)
# for this checkout's other builders and harness consumers, e.g. a transient
# rustc from rust-analyzer, before sweeping or building.
require_idle() {
  until idle_now; do
    if (( ${waited:-0} >= ${wait_seconds:-0} )); then
      echo "build-budget: refusing to sweep/build while another builder or harness consumer is active: $busy" >&2
      return 1
    fi
    sleep "$poll_seconds"
    waited=$(( ${waited:-0} + poll_seconds ))
  done
}
target_has_open_files() {
  local open_files
  if ! command -v lsof >/dev/null; then
    sweep_skip_reason=open-file-scan-unavailable
    echo 'build-budget: lsof unavailable; treating target as in use' >&2
    return 0
  fi
  if ! open_files="$(lsof -n -P 2>/dev/null)"; then
    sweep_skip_reason=open-file-scan-failed
    echo 'build-budget: open-file scan failed; treating target as in use' >&2
    return 0
  fi
  if ! command -v grep >/dev/null; then
    sweep_skip_reason=open-file-scan-unavailable
    echo 'build-budget: grep unavailable; treating target as in use' >&2
    return 0
  fi
  sweep_skip_reason=open-files
  grep -F -q -- "$root/target/" <<< "$open_files"
}
sweep() {
  local mode="$1" rc=0 before=null after=null outcome=completed reason=success
  [[ "$mode" == dry ]] || before="$(target_kib strict)" || before=null
  sweep_original "$mode" || rc=$?
  [[ "$mode" == dry ]] && return "$rc"
  after="$(target_kib strict)" || after=null
  if (( rc == 3 )); then outcome=skipped; reason="$sweep_skip_reason";
  elif (( rc != 0 )); then
    outcome=failed; reason="$sweep_failure_reason"
    [[ "$reason" != active-consumer ]] || outcome=refused
  fi
  aura_receipt_record_cleanup "$root" sweep target "$outcome" "$reason" "$rc" "$before" "$after" null null
  return "$rc"
}
sweep_original() {
  local mode="$1" preview release_count incoming_count incoming_path rc
  sweep_skip_reason=unknown
  sweep_failure_reason=preview
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
  incoming_count=0
  case "$preserve_cache_lane" in
    wasm-debug) incoming_path='target/wasm32-unknown-unknown/debug/' ;;
    *) incoming_path='' ;;
  esac
  if [[ -n "$incoming_path" ]]; then
    incoming_count="$(rg -c -F "$incoming_path" "$preview" || true)"
    incoming_count="${incoming_count:-0}"
  fi
  rm -f "$preview"
  [[ "$mode" == dry ]] && return 0
  if (( release_count > 0 || incoming_count > 0 || allow_live_harness == 1 )); then
    if (( release_count > 0 )); then sweep_skip_reason=protected-production;
    elif (( incoming_count > 0 )); then sweep_skip_reason=protected-incoming-lane;
    else sweep_skip_reason=live-harness-mode; fi
    echo 'build-budget: preserving protected caches and live harness artifacts; skipping global sweep' >&2
    return 3
  fi
  sweep_failure_reason=active-consumer
  require_idle || return $?
  if target_has_open_files; then
    echo 'build-budget: target has open files; skipping global sweep' >&2
    return 3
  fi
  printf 'Sweep: cargo sweep --maxsize %sGiB .\n' "$cap_gib"
  sweep_failure_reason=cargo-sweep
  (cd "$root" && cargo sweep --maxsize "${cap_gib}GiB" .)
}
recover_cache() {
  local sweep_status
  if (( $(target_kib strict) <= cap_kib && $(free_kib) - incoming_reservation_kib >= min_free_kib )); then
    return 0
  fi
  # Reclaim isolated, inactive caches before considering the shared debug
  # dependency tree. The target cap is soft while the disk floor is healthy.
  prune_safe_lanes apply || return $?
  if (( $(free_kib) < min_free_kib )); then
    if sweep apply; then :;
    else
      sweep_status=$?
      (( sweep_status == 3 )) || return "$sweep_status"
    fi
  fi
  if (( $(target_kib strict) > cap_kib )); then
    echo 'build-budget: target remains above soft cap after guarded inactive-lane recovery; reusable dependencies retained' >&2
  fi
}
prune_safe_lanes() {
  local mode="$1" scope=soft-cap priority size candidate_root candidate
  if (( $(free_kib) - incoming_reservation_kib < min_free_kib )); then scope=headroom; fi
  while IFS=$'\t' read -r priority size candidate_root candidate; do
    if (( allow_live_harness == 1 )) &&
      { [[ "$candidate_root" != "$root" ]] || [[ "$candidate" != wasm-debug ]]; }; then
      continue
    fi
    printf 'Cache candidate: scope=%s worktree=%s lane=%s measured=%s KiB\n' \
      "$scope" "$candidate_root" "$candidate" "$size"
    if [[ "$mode" == dry ]]; then
      bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$candidate_root" --lane "$candidate" --dry-run
    else
      require_idle || return $?
      local -a owner_args=()
      [[ "$candidate_root" != "$root" ]] || owner_args=(--lock-owned-by "$$")
      prune_status=0
      bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$candidate_root" \
        --lane "$candidate" --apply "${owner_args[@]+"${owner_args[@]}"}" || prune_status=$?
      if [[ "$candidate_root" != "$root" ]]; then
        aura_receipt_update --arg worktree "$candidate_root" --arg lane "$candidate" \
          --argjson status "$prune_status" --arg measured "$size" \
          --argjson free "$(free_kib)" \
          '.cleanup_events += [{stage:"admission",operation:"guarded-sibling-lane",
            worktree:$worktree,lane:$lane,exit_status:$status,
            outcome:(if $status==0 then "completed" else "refused" end),
            measured_candidate_kib:($measured|tonumber),free_after_kib:$free}]'
      fi
      if (( prune_status != 0 )); then
        echo "build-budget: $candidate remains protected or busy" >&2
        require_idle || return $?
      fi
      if [[ "$scope" == headroom ]] && (( $(free_kib) - incoming_reservation_kib >= min_free_kib )); then
        break
      fi
      if (( $(target_kib) <= cap_kib && $(free_kib) >= min_free_kib )); then
        break
      fi
    fi
  done < <(aura_cache_rank_candidates "$root" "$scope" "$preserve_cache_lane" "$CARGO_INCREMENTAL" |
    sort -t $'\t' -k1,1n -k2,2nr)
}

printf 'Budget: target=%s GiB soft; admission=%s GiB free; emergency=%s GiB free\n' \
  "$cap_gib" "$min_free_gib" "$emergency_gib"
if (( dry_run == 0 )); then
  profile="${AURA_BUILD_PROFILE:-unlabelled}"
  features="${AURA_BUILD_FEATURES:-unlabelled}"
  target_triple="${AURA_BUILD_TARGET_TRIPLE:-$(rustc -vV 2>/dev/null | awk '/^host: / {print $2}' || true)}"
  source "$repo_root/scripts/dev/build-receipt.sh"
  aura_receipt_init "$root" "$lane" "$@"
  estimate_incoming_growth
  aura_receipt_update --argjson incoming "$incoming_reservation_kib" \
    '.admission.incoming_reservation_kib=$incoming'
  export AURA_BUILD_CLEANUP_STAGE=pre-admission
  trap 'receipt_status=$?; aura_receipt_finish "$receipt_status"' EXIT
fi
printf 'Build policy: CARGO_INCREMENTAL=%s\n' "$CARGO_INCREMENTAL"
before_free="$(free_kib)"
before_target="$(target_kib strict)"
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

# Gates opt in to waiting (AURA_BUILD_WAIT_SECONDS) for the checkout's lock
# and for other builders such as rust-analyzer, instead of failing at once.
wait_seconds="${AURA_BUILD_WAIT_SECONDS:-0}"
[[ "$wait_seconds" =~ ^[0-9]+$ ]] || { echo 'build-budget: AURA_BUILD_WAIT_SECONDS must be a whole number' >&2; exit 2; }
waited=0
lock_dir="$root/target/.aura-build-budget.lock"
mkdir -p "$root/target"
holds_lock=0
acquire_checkout_lock() {
  until mkdir "$lock_dir" 2>/dev/null; do
    # A holder killed before its EXIT trap leaves its lock behind (Task 216):
    # reclaim a lock whose recorded pid is no longer alive, as the admission
    # lock does. A lock without a pid yet may be a holder starting up.
    holder="$(cat "$lock_dir/pid" 2>/dev/null || true)"
    if [[ "$holder" =~ ^[0-9]+$ ]] && ! kill -0 "$holder" 2>/dev/null; then
      echo "build-budget: reclaiming $lock_dir from dead holder pid $holder" >&2
      rm -f "$lock_dir/pid"
      rmdir "$lock_dir" 2>/dev/null || true
      continue
    fi
    if (( waited >= wait_seconds )); then
      echo "build-budget: another budgeted build holds $lock_dir (or a stale lock needs review)" >&2
      exit 1
    fi
    sleep "$poll_seconds"
    waited=$((waited + poll_seconds))
  done
  printf '%s\n' "$$" > "$lock_dir/pid"
  holds_lock=1
}
release_checkout_lock() {
  (( holds_lock == 1 )) || return 0
  rm -f "$lock_dir/pid"
  rmdir "$lock_dir" 2>/dev/null || true
  holds_lock=0
}
acquire_checkout_lock
child_pid=''
command_log_fd=''
command_tee_pid=''
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
  local exit_status=$?
  if [[ -n "$child_pid" ]] && kill -0 "$child_pid" 2>/dev/null; then
    stop_owned_group "$child_pid"
    wait "$child_pid" 2>/dev/null || true
  fi
  if [[ -n "$command_log_fd" ]]; then
    exec {command_log_fd}>&-
    command_log_fd=''
  fi
  if [[ -n "$command_tee_pid" ]]; then
    wait "$command_tee_pid" 2>/dev/null || true
    command_tee_pid=''
  fi
  aura_receipt_update --argjson status "$exit_status" \
    --arg finished "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    'if .command_phase!=null and .command_phase.finished_utc==null then
      .command_phase.finished_utc=$finished | .command_phase.exit_status=$status |
      .phases += [.command_phase]
     else . end'
  [[ -z "$reservation" ]] || rm -f "$reservation"
  release_checkout_lock
  # A signal can exit before the normal report path. Retain its observed
  # drawdown so the next admission does not mistake an interrupted build for
  # a zero-growth lane.
  if [[ "${pre_free:-}" =~ ^[0-9]+$ && "${pre_target:-}" =~ ^[0-9]+$ ]]; then
    local exit_free exit_target
    exit_free="$(free_kib)"
    exit_target="$(target_kib)"
    lowest_free="${lowest_free:-$pre_free}"
    peak_target="${peak_target:-$pre_target}"
    (( exit_free < lowest_free )) && lowest_free="$exit_free"
    (( exit_target > peak_target )) && peak_target="$exit_target"
    aura_receipt_update --argjson pre_free "$pre_free" --argjson pre_target "$pre_target" \
      --argjson lowest_free "$lowest_free" --argjson peak_target "$peak_target" \
      '.disk={pre_free_kib:$pre_free,pre_target_kib:$pre_target,
        lowest_free_kib:$lowest_free,peak_target_kib:$peak_target,
        measured_growth_kib:([0,($pre_free-$lowest_free),($peak_target-$pre_target)]|max)}'
  fi
  aura_receipt_finish "$exit_status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

require_idle || exit $?
if (( before_target > cap_kib || before_free < min_free_kib )); then
  if (( no_prune == 1 )); then
    if (( before_free < min_free_kib )); then
      echo 'build-budget: no-prune mode cannot recover the required headroom' >&2
      exit 1
    fi
    echo 'build-budget: no-prune mode retains target above soft cap with healthy disk headroom' >&2
  else
    require_idle
    recover_cache || exit $?
  fi
fi
pre_free="$(free_kib)"
pre_target="$(target_kib strict)"
printf 'Pre-build: free=%s KiB target=%s KiB\n' "$pre_free" "$pre_target"
until admit_build; do
  (( waited < wait_seconds )) || exit 1
  if (( no_prune == 0 )); then
    prune_safe_lanes apply || exit $?
  fi
  # Free space comes from prune-inactive-lane, which needs this checkout's
  # lock; release it while waiting for admission (Task 218: run 175's ship
  # held it and deadlocked the prune that would have admitted it).
  release_checkout_lock
  sleep "$poll_seconds"
  waited=$((waited + poll_seconds))
  acquire_checkout_lock
done

started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
printf 'Build lane=%s command:' "$lane"
printf ' %q' "$@"
printf '\n'
aura_receipt_update --arg started "$started" \
  '.current_phase="command" | .command_phase={phase:"command",started_utc:$started,
    finished_utc:null,exit_status:null}'
export AURA_BUILD_CLEANUP_STAGE=command
set -m # Give the background build its own process group for bounded shutdown.
exec {command_log_fd}> >(tee -a "$AURA_BUILD_COMMAND_LOG")
command_tee_pid=$!
(cd "$root" && AURA_BUILD_BUDGET_HELD="$root" "$@") >&"$command_log_fd" 2>&1 &
child_pid=$!
set +m
aura_receipt_update --argjson pid "$child_pid" \
  --arg identity "$(ps -p "$child_pid" -o lstart= 2>/dev/null || true)" \
  '.child={pid:$pid,start_identity:$identity}'
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
exec {command_log_fd}>&-
command_log_fd=''
tee_status=0
wait "$command_tee_pid" || tee_status=$?
command_tee_pid=''
(( tee_status == 0 || status != 0 )) || status=77
aura_receipt_update --argjson status "$status" --arg finished "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  '.command_exit_status=$status | .current_phase="cache-cleanup" |
    .command_phase.finished_utc=$finished | .command_phase.exit_status=$status |
    .phases += [.command_phase]'
post_build_target="$(target_kib)"
post_build_free="$(free_kib)"
(( post_build_target > peak_target )) && peak_target="$post_build_target"

# The build is done: its post-build sweep never waits for other builders.
wait_seconds=0
export AURA_BUILD_CLEANUP_STAGE=post-command
cache_cleanup=not-run-command-failed
if (( status == 0 && no_prune == 1 )); then
  cache_cleanup=skipped-no-prune
  echo 'No-prune mode: post-build cache collection skipped'
elif (( status == 0 && post_build_target <= cap_kib && post_build_free >= min_free_kib )); then
  cache_cleanup=skipped-healthy
  echo 'Post-build: target below soft cap and disk floor healthy; retaining reusable artifacts'
elif (( status == 0 )); then
  # Cleanup is best effort: a sweep skipped because another builder is (or
  # became) active leaves the build's own status (Task 212). Only a genuine
  # sweep failure changes it.
  if idle_now; then
    cache_cleanup=attempted
    recovery_command=recover_cache
    if "$recovery_command" apply; then :;
    else
      sweep_status=$?
      if (( sweep_status == 3 )); then
        prune_safe_lanes apply || true
      elif ! idle_now; then
        cache_cleanup=skipped-busy
        echo 'build-budget: post-build sweep skipped because another builder started' >&2
      else
        cache_cleanup=failed
        echo 'build-budget: post-build sweep failed' >&2
        status=76
      fi
    fi
  else
    cache_cleanup=skipped-busy
    echo 'build-budget: post-build sweep skipped because another builder started' >&2
  fi
fi
after_free="$(free_kib)"
after_target="$(target_kib)"
aura_receipt_update --arg result "$cache_cleanup" \
  --argjson free "$after_free" --argjson target "$after_target" --argjson cap "$cap_kib" \
  '.cleanup={result:$result,after_free_kib:$free,after_target_kib:$target,
    target_above_soft_cap:($target>$cap)} | .current_phase="report"'
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
aura_receipt_update --arg disk_log "$log_path" --arg report "${report_path:-}" \
  --argjson pre_free "$pre_free" --argjson pre_target "$pre_target" \
  --argjson lowest_free "$lowest_free" --argjson peak_target "$peak_target" \
  '.artifacts += {disk_log:$disk_log,disk_report:(if $report=="" then null else $report end)} |
   .disk={pre_free_kib:$pre_free,pre_target_kib:$pre_target,
     lowest_free_kib:$lowest_free,peak_target_kib:$peak_target,
     measured_growth_kib:([0,($pre_free-$lowest_free),($peak_target-$pre_target)]|max)}'
exit "$status"
