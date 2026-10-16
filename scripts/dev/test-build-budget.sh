#!/usr/bin/env bash
# Isolated safety tests; no real Cargo build or repository target cleanup.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-build-budget-test.XXXXXX")"
test_root="$(cd "$test_root" && pwd -P)"
trap 'rm -rf "$test_root"' EXIT
fakebin="$test_root/fakebin"
project="$test_root/project"
mkdir -p "$fakebin" "$project/target"
: > "$project/Cargo.toml"
export FREE_FILE="$test_root/free_kib"
export SIZE_FILE="$test_root/size_kib"
export CALLS_FILE="$test_root/calls"
export ACTIVE_FILE="$test_root/active"
export ZOMBIE_PID_FILE="$test_root/zombie-pid"
export FAIL_SWEEP_FILE="$test_root/fail-sweep"
export PROTECTED_FILE="$test_root/protected-release"
export PROTECTED_WEB_FILE="$test_root/protected-web"
export RACE_ON_PREVIEW_FILE="$test_root/race-on-preview"
export OPEN_TARGET_FILE="$test_root/open-target"
export DU_FAIL_ONCE_FILE="$test_root/du-fail-once"
export DU_EMPTY_FILE="$test_root/du-empty"
export INCREMENTAL_SIZE_FILE="$test_root/incremental-size"
export WASM_SIZE_FILE="$test_root/wasm-size"
export DYLINT_SIZE_FILE="$test_root/dylint-size"
export TRYBUILD_SIZE_FILE="$test_root/trybuild-size"
export EVICT_ON_SWEEP_FILE="$test_root/evict-on-sweep"
export PREVIEW_INCOMING_FILE="$test_root/preview-incoming"
export AURA_BUILD_TARGET_CAP_GIB=8
# Cases choose their own waits; a caller's AURA_BUILD_WAIT_SECONDS (gates and
# ship.sh export one) must not turn an expected refusal into a long wait.
export AURA_BUILD_WAIT_SECONDS=0
unset AURA_BUILD_BUDGET_HELD
export PROJECT_ROOT="$project"
export AURA_BUILD_SHARED_DIR="$test_root/shared"
export PATH="$fakebin:$PATH"

cat > "$fakebin/df" <<'EOF'
#!/usr/bin/env bash
printf 'Filesystem 1024-blocks Used Available Capacity Mounted\n'
printf 'fake 99999999 0 %s 0%% /\n' "$(cat "$FREE_FILE")"
EOF
cat > "$fakebin/du" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$DU_EMPTY_FILE" ]] || exit 1
size="$(cat "$SIZE_FILE")"
if [[ "${@: -1}" == "$PROJECT_ROOT/target/debug/incremental" && -f "$INCREMENTAL_SIZE_FILE" ]]; then
  printf '%s\t%s\n' "$(cat "$INCREMENTAL_SIZE_FILE")" "${@: -1}"
  exit 0
fi
if [[ "${@: -1}" == "$PROJECT_ROOT/target/wasm32-unknown-unknown/debug" && -f "$WASM_SIZE_FILE" ]]; then
  printf '%s\t%s\n' "$(cat "$WASM_SIZE_FILE")" "${@: -1}"
  exit 0
fi
if [[ "${@: -1}" == "$PROJECT_ROOT/target/dylint" && -f "$DYLINT_SIZE_FILE" ]]; then
  printf '%s\t%s\n' "$(cat "$DYLINT_SIZE_FILE")" "${@: -1}"
  exit 0
fi
if [[ "${@: -1}" == "$PROJECT_ROOT/target/tests/trybuild" && -f "$TRYBUILD_SIZE_FILE" ]]; then
  printf '%s\t%s\n' "$(cat "$TRYBUILD_SIZE_FILE")" "${@: -1}"
  exit 0
fi
if [[ -f "$INCREMENTAL_SIZE_FILE" && ! -d "$PROJECT_ROOT/target/debug/incremental" ]]; then
  size=$((size - $(cat "$INCREMENTAL_SIZE_FILE")))
fi
if [[ -f "$WASM_SIZE_FILE" && ! -d "$PROJECT_ROOT/target/wasm32-unknown-unknown/debug" ]]; then
  size=$((size - $(cat "$WASM_SIZE_FILE")))
fi
if [[ -f "$DYLINT_SIZE_FILE" && ! -d "$PROJECT_ROOT/target/dylint" ]]; then
  size=$((size - $(cat "$DYLINT_SIZE_FILE")))
fi
if [[ -f "$TRYBUILD_SIZE_FILE" && ! -d "$PROJECT_ROOT/target/tests/trybuild" ]]; then
  size=$((size - $(cat "$TRYBUILD_SIZE_FILE")))
fi
printf '%s\t%s\n' "$size" "${@: -1}"
if [[ -f "$DU_FAIL_ONCE_FILE" ]]; then
  rm -f "$DU_FAIL_ONCE_FILE"
  echo 'du: transient disappearing Cargo artifact' >&2
  exit 1
fi
EOF
cat > "$fakebin/ps" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == -p && "${3:-}" == -o && "${4:-}" == stat= &&
  -f "$ZOMBIE_PID_FILE" && "${2:-}" == "$(cat "$ZOMBIE_PID_FILE")" ]]; then
  printf 'Z\n'
  exit 0
fi
[[ ! -f "$ACTIVE_FILE" ]] || printf '999 %s --out-dir %s/target/debug/deps\n' "$(cat "$ACTIVE_FILE")" "${ACTIVE_ROOT:-$PROJECT_ROOT}"
EOF
cat > "$fakebin/lsof" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$OPEN_TARGET_FILE" ]] || cat "$OPEN_TARGET_FILE"
EOF
cat > "$fakebin/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$CALLS_FILE"
if [[ -f "$PROTECTED_FILE" && " $* " == *' --dry-run '* ]]; then
  printf '[DEBUG] Would remove: "%s/target/release/deps/libproduction.rlib"\n' "$PWD"
fi
if [[ -f "$PROTECTED_WEB_FILE" && " $* " == *' --dry-run '* ]]; then
  printf '[DEBUG] Would remove: "%s/target/wasm-release/deps/libweb.rlib"\n' "$PWD"
fi
if [[ -f "$PREVIEW_INCOMING_FILE" && " $* " == *' --dry-run '* ]]; then
  printf '[DEBUG] Would remove: "%s/target/wasm32-unknown-unknown/debug/deps/libincoming.rlib"\n' "$PWD"
fi
if [[ -f "$RACE_ON_PREVIEW_FILE" && " $* " == *' --dry-run '* ]]; then
  printf 'cargo\n' > "$ACTIVE_FILE"
fi
if [[ -f "$FAIL_SWEEP_FILE" ]]; then exit 1; fi
if [[ -f "$EVICT_ON_SWEEP_FILE" && " $* " != *' --dry-run '* ]]; then
  rm -f "$PROJECT_ROOT/target/debug/deps/reusable.rlib"
fi
EOF
chmod +x "$fakebin"/*

reset_case() {
  rm -rf "$project/artifacts/disk-budget/receipts"
  rm -f "$project/artifacts/disk-budget/builds.tsv"
  printf '%s\n' $((20 * 1024 * 1024)) > "$FREE_FILE"
  printf '%s\n' $((6 * 1024 * 1024)) > "$SIZE_FILE"
  : > "$CALLS_FILE"
  rm -f "$ACTIVE_FILE" "$ZOMBIE_PID_FILE" "$FAIL_SWEEP_FILE" "$PROTECTED_FILE" "$PROTECTED_WEB_FILE" "$PREVIEW_INCOMING_FILE" "$RACE_ON_PREVIEW_FILE" "$OPEN_TARGET_FILE" "$DU_FAIL_ONCE_FILE" "$DU_EMPTY_FILE" "$EVICT_ON_SWEEP_FILE"
  rm -rf "$project/target/.aura-build-budget.lock"
  rm -f "$project/target/debug/deps/reusable.rlib"
  rmdir "$project/target/debug/deps" "$project/target/debug" 2>/dev/null || true
  rm -f "$INCREMENTAL_SIZE_FILE" "$WASM_SIZE_FILE" "$DYLINT_SIZE_FILE" "$TRYBUILD_SIZE_FILE"
}

run_budget() {
  bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" --lane test "$@"
}

expect_status() {
  local expected="$1" actual
  shift
  if "$@" > "$test_root/output" 2>&1; then actual=0; else actual=$?; fi
  if [[ "$actual" -ne "$expected" ]]; then
    cat "$test_root/output" >&2
    echo "expected exit $expected, got $actual" >&2
    exit 1
  fi
}

reset_case
expect_status 0 run_budget -- sh -c 'exit 0'
[[ ! -s "$CALLS_FILE" ]] # Healthy under-cap target needs no sweep.
[[ ! -d "$project/target/.aura-build-budget.lock" ]]
[[ "$(awk -F '\t' 'NR == 2 {print NF}' "$project/artifacts/disk-budget/builds.tsv")" -eq 18 ]]
rg -l 'Build context: lane=test' "$project"/artifacts/disk-budget/report.* >/dev/null
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.final_exit_status==0 and .command_exit_status==0 and .current_phase=="finished"
  and (.phases|length)==1 and .phases[0].phase=="command" and
  .phases[0].started_utc!=null and .phases[0].finished_utc!=null and
  .phases[0].exit_status==0 and .cleanup.result=="skipped-healthy" and .child.pid!=null
  and .artifacts.disk_report!=null and .artifacts.command_log!=null and
  .notification!=null and .admission.admitted==true' "$receipt" >/dev/null
command_log="$(jq -r '.artifacts.command_log' "$receipt")"
[[ -f "$command_log" && "$(find "$command_log" -perm 0600 -print)" == "$command_log" ]]
jq -e --arg receipt "$receipt" '.receipt==$receipt and .exit_status==0' \
  "$(dirname "$receipt")/completion.json" >/dev/null

# The pre-commit hook invokes the wrapper with the host Bash (3.2 on macOS).
# Verify the child really runs and its output is flushed before completion.
reset_case
host_bash=/bin/bash
[[ -x "$host_bash" ]] || host_bash="$(command -v bash)"
expect_status 37 "$host_bash" "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane test --no-prune -- sh -c 'touch "$PROJECT_ROOT/host-command-ran"; printf "host-child-marker\n"; exit 37'
[[ -f "$project/host-command-ran" ]]
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
command_log="$(jq -r '.artifacts.command_log' "$receipt")"
[[ -f "$command_log" && "$(find "$command_log" -perm 0600 -print)" == "$command_log" ]]
rg -q '^host-child-marker$' "$command_log"
jq -e '.command_exit_status==37 and .final_exit_status==37 and
  .current_phase=="finished" and .phases[0].exit_status==37' "$receipt" >/dev/null
jq -e --arg receipt "$receipt" '.receipt==$receipt and .exit_status==37' \
  "$(dirname "$receipt")/completion.json" >/dev/null

# Commands and their descendants must not inherit the wrapper's private FIFO
# writer. A compiler daemon can detach and close stdout/stderr but retain that
# extra descriptor, indefinitely withholding the receipt's terminal event.
reset_case
expect_status 37 timeout 15s "$host_bash" "$repo_root/scripts/dev/build-budget.sh" \
  --root "$project" --lane test --no-prune -- "$host_bash" -c '
  if { : >&9; } 2>/dev/null; then exit 87; fi
  "$1" -c '\''if { : >&9; } 2>/dev/null; then exit 88; fi'\'' || exit $?
  printf "private-fifo-not-inherited\n"
  exit 37' fixture "$host_bash"
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.final_exit_status==37 and .command_exit_status==37 and
  .current_phase=="finished" and .notification!=null' "$receipt" >/dev/null
rg -q '^private-fifo-not-inherited$' "$(jq -r '.artifacts.command_log' "$receipt")"
[[ ! -d "$project/target/.aura-build-budget.lock" ]]
[[ ! -e "$(dirname "$receipt")/command.pipe" ]]

# `kill -0` can still see an exited, unreaped child. Simulate that state and
# require a terminal failure receipt and notification before the timeout.
reset_case
kill() {
  if [[ "${1:-}" == -0 && -f "$ZOMBIE_PID_FILE" && "${2:-}" == "$(cat "$ZOMBIE_PID_FILE")" ]]; then
    return 0
  fi
  builtin kill "$@"
}
export -f kill
expect_status 37 timeout 15s bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane test --no-prune -- sh -c 'printf "%s\n" "$$" > "$ZOMBIE_PID_FILE"; exit 37'
unset -f kill
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.final_exit_status==37 and .command_exit_status==37 and
  .current_phase=="finished" and .phases[0].phase=="command" and
  .phases[0].exit_status==37 and .notification!=null' "$receipt" >/dev/null
jq -e --arg receipt "$receipt" '.receipt==$receipt and .exit_status==37' \
  "$(dirname "$receipt")/completion.json" >/dev/null

# Failure details stay in a protected artifact; the compact receipt and
# terminal notification disclose only typed status, never command output.
reset_case
expect_status 37 env FIXTURE_SECRET=fixture-private bash "$repo_root/scripts/dev/build-budget.sh" \
  --root "$project" --lane test -- sh -c '
  printf "test first_failure ... FAILED\npanicked at src/first.rs:42\n"
  printf "assertion left=$FIXTURE_SECRET\n"
  for index in $(seq 1 100); do printf "diagnostic %s\n" "$index"; done
  exit 37'
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.final_exit_status==37 and
  .failure_excerpt=="test first_failure failed at src/first.rs:42; details in protected log"' "$receipt" >/dev/null
! rg -q 'fixture-private' "$receipt" "$(dirname "$receipt")/completion.json"
rg -q 'fixture-private' "$(jq -r '.artifacts.command_log' "$receipt")"

# A successful build below the soft cap must retain useful dependency objects
# even when cargo sweep would choose them as old. The hard disk floor is safe.
reset_case
export AURA_BUILD_TARGET_CAP_GIB=10
printf '%s\n' $((9 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((40 * 1024 * 1024)) > "$FREE_FILE"
mkdir -p "$project/target/debug/deps"
printf 'reusable\n' > "$project/target/debug/deps/reusable.rlib"
touch "$EVICT_ON_SWEEP_FILE"
expect_status 0 run_budget -- sh -c 'test -f target/debug/deps/reusable.rlib'
[[ ! -s "$CALLS_FILE" && -f "$project/target/debug/deps/reusable.rlib" ]]
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.cleanup.result=="skipped-healthy" and .cleanup.target_above_soft_cap==false and
  all(.cleanup_events[]; .operation!="sweep") and .final_exit_status==0' "$receipt" >/dev/null
export AURA_BUILD_TARGET_CAP_GIB=8

reset_case
expect_status 0 env -u CARGO_INCREMENTAL bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" --no-prune -- sh -c 'test "$CARGO_INCREMENTAL" = 0'
rg -q 'Build policy: CARGO_INCREMENTAL=0' "$test_root/output"
reset_case
expect_status 0 env CARGO_INCREMENTAL=1 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" --no-prune -- sh -c 'test "$CARGO_INCREMENTAL" = 1'
rg -q 'Build policy: CARGO_INCREMENTAL=1' "$test_root/output"
reset_case
expect_status 2 env CARGO_INCREMENTAL=invalid bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" --no-prune -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" ]]

reset_case
expect_status 0 run_budget --no-prune -- sh -c 'exit 0'
[[ ! -s "$CALLS_FILE" ]]
rg -q 'post-build cache collection skipped' "$test_root/output"

reset_case
expect_status 0 run_budget --no-prune -- sh -c 'touch "$DU_FAIL_ONCE_FILE"'
[[ ! -f "$DU_FAIL_ONCE_FILE" ]]
rg -q 'exit=0' "$test_root/output"

reset_case
touch "$DU_EMPTY_FILE"
expect_status 1 run_budget -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" ]]
rg -q 'could not measure target size' "$test_root/output"

reset_case
expect_status 0 run_budget --no-prune -- sh -c 'printf "cargo\n" > "$ACTIVE_FILE"'
[[ ! -s "$CALLS_FILE" ]]

reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
expect_status 0 run_budget --no-prune -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ -e "$FREE_FILE.build-ran" && ! -s "$CALLS_FILE" ]]
rg -q 'above soft cap' "$test_root/output"
rm -f "$FREE_FILE.build-ran"

reset_case
rmdir "$project/target"
ln -s "$test_root" "$project/target"
expect_status 1 run_budget -- sh -c 'exit 0'
rm "$project/target"
mkdir "$project/target"

reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
mkdir -p "$project/target/debug/deps"
printf 'reusable\n' > "$project/target/debug/deps/reusable.rlib"
expect_status 0 run_budget -- sh -c 'exit 0'
[[ ! -s "$CALLS_FILE" && -f "$project/target/debug/deps/reusable.rlib" ]]
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.cleanup.target_above_soft_cap==true and
  all(.cleanup_events[]; .operation!="sweep")' "$receipt" >/dev/null

reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((14 * 1024 * 1024)) > "$FREE_FILE"
touch "$RACE_ON_PREVIEW_FILE"
expect_status 1 run_budget -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" ]]
[[ "$(wc -l < "$CALLS_FILE")" -eq 1 ]]
rg -q -- '--dry-run' "$CALLS_FILE"
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

reset_case
printf '%s\n' $((6 * 1024 * 1024)) > "$SIZE_FILE"
mkdir -p "$project/target/release" "$project/target/wasm32-unknown-unknown/debug"
printf 'loaded\n' > "$project/target/release/loaded"
printf 'stale\n' > "$project/target/wasm32-unknown-unknown/debug/cache"
printf 'fake 123 %s\n' "$project/target/release/loaded" > "$OPEN_TARGET_FILE"
expect_status 0 run_budget -- sh -c 'printf "%s\n" $((14 * 1024 * 1024)) > "$FREE_FILE"'
if rg -v -- '--dry-run' "$CALLS_FILE" | rg -q .; then
  echo 'open target file was swept' >&2
  exit 1
fi
[[ -f "$project/target/release/loaded" ]]
[[ ! -e "$project/target/wasm32-unknown-unknown/debug" ]]

reset_case
printf '%s\n' $((6 * 1024 * 1024)) > "$SIZE_FILE"
touch "$PROTECTED_FILE"
mkdir -p "$project/target/release" "$project/target/wasm32-unknown-unknown/debug"
printf 'keep\n' > "$project/target/release/production"
printf 'stale\n' > "$project/target/wasm32-unknown-unknown/debug/cache"
expect_status 0 run_budget -- sh -c 'printf "%s\n" $((14 * 1024 * 1024)) > "$FREE_FILE"'
if rg -v -- '--dry-run' "$CALLS_FILE" | rg -q .; then
  echo 'protected release cache was swept' >&2
  exit 1
fi
rg -q 'Protected production/web candidates: 1' "$test_root/output"
[[ -f "$project/target/release/production" ]]
[[ ! -e "$project/target/wasm32-unknown-unknown/debug" ]]

reset_case
printf '%s\n' $((6 * 1024 * 1024)) > "$SIZE_FILE"
touch "$PROTECTED_WEB_FILE"
expect_status 0 run_budget -- sh -c 'printf "%s\n" $((14 * 1024 * 1024)) > "$FREE_FILE"'
if rg -v -- '--dry-run' "$CALLS_FILE" | rg -q .; then
  echo 'protected web cache was swept' >&2
  exit 1
fi
rg -q 'Protected production/web candidates: 1' "$test_root/output"

reset_case
expect_status 0 run_budget --dry-run
[[ "$(wc -l < "$CALLS_FILE")" -eq 1 ]]
rg -q -- '--dry-run' "$CALLS_FILE"
[[ ! -d "$project/target/.aura-build-budget.lock" ]]
rg -q 'Admission preview: command absent; incoming reservation unknown' "$test_root/output"

# Preview mirrors admission arithmetic without creating a receipt, lock,
# reservation, command result, or stale-reservation cleanup side effect.
reset_case
mkdir -p "$AURA_BUILD_SHARED_DIR/reservations"
printf '%s\n' 2097152 > "$AURA_BUILD_SHARED_DIR/reservations/99999999"
sleep 60 & preview_holder=$!
printf '%s\n' 1048576 > "$AURA_BUILD_SHARED_DIR/reservations/$preview_holder"
printf '%s\n' $((19 * 1024 * 1024)) > "$FREE_FILE"
expect_status 0 run_budget --dry-run --no-prune -- sh -c 'touch "$FREE_FILE.preview-command-ran"'
rg -q 'other-reservations=1048576 KiB incoming-reservation=4194304 KiB floor=15728640 KiB margin=-1048576 KiB outcome=refused identity=generic workload=generic' "$test_root/output"
[[ -f "$AURA_BUILD_SHARED_DIR/reservations/99999999" &&
   -f "$AURA_BUILD_SHARED_DIR/reservations/$preview_holder" &&
   ! -e "$FREE_FILE.preview-command-ran" &&
   ! -d "$AURA_BUILD_SHARED_DIR/admission.lock" &&
   ! -d "$project/artifacts/disk-budget/receipts" ]]
kill "$preview_holder"
wait "$preview_holder" 2>/dev/null || true
rm -f "$AURA_BUILD_SHARED_DIR/reservations/99999999" "$AURA_BUILD_SHARED_DIR/reservations/$preview_holder"

# An oversized unused incremental lane is reclaimed before dependencies are
# swept; the running command observes both admission size and dependency reuse.
reset_case
printf '%s\n' $((17 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((11 * 1024 * 1024)) > "$INCREMENTAL_SIZE_FILE"
mkdir -p "$project/target/debug/incremental" "$project/target/debug/deps"
touch "$project/target/debug/deps/reusable.rlib"
expect_status 0 env CARGO_INCREMENTAL=0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" -- \
  sh -c 'test ! -d target/debug/incremental && test -f target/debug/deps/reusable.rlib && test ! -s "$CALLS_FILE"'
[[ ! -s "$CALLS_FILE" ]] # Inactive cache suffices; preserve dependencies.
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.cleanup_events[0] | .stage=="pre-admission" and .operation=="guarded-lane"
  and .lane=="debug-incremental" and .outcome=="removed" and .exit_status==0
  and .before_kib==11534336 and .after_kib==0 and .reclaimed_kib==11534336' "$receipt" >/dev/null

# A failed command must retain positive pre-admission cleanup evidence even
# though its post-command cleanup summary correctly says not run.
reset_case
printf '%s\n' $((17 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((11 * 1024 * 1024)) > "$INCREMENTAL_SIZE_FILE"
mkdir -p "$project/target/debug/incremental"
expect_status 37 env CARGO_INCREMENTAL=0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" -- sh -c 'exit 37'
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.final_exit_status==37 and .cleanup.result=="not-run-command-failed" and
  any(.cleanup_events[];.stage=="pre-admission" and .outcome=="removed" and .reclaimed_kib==11534336)' "$receipt" >/dev/null

# Inactive wasm is reclaimed before any dependency sweep.
reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((5 * 1024 * 1024)) > "$WASM_SIZE_FILE"
mkdir -p "$project/target/wasm32-unknown-unknown/debug"
mkdir -p "$project/target/debug/deps"
touch "$project/target/debug/deps/reusable.rlib"
expect_status 0 run_budget -- sh -c 'test ! -d target/wasm32-unknown-unknown/debug && test -f target/debug/deps/reusable.rlib'
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e 'all(.cleanup_events[]; .operation!="sweep" or .stage!="pre-admission") and
  any(.cleanup_events[];.operation=="guarded-lane" and .lane=="wasm-debug" and
  .outcome=="removed" and .before_kib==5242880 and .after_kib==0)' "$receipt" >/dev/null

# A declared incoming wasm lane survives pre-admission and post-command cache
# recovery. Other inactive lanes remain reclaimable, and a hard floor cannot
# authorize a global sweep that would evict the incoming lane.
reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((5 * 1024 * 1024)) > "$WASM_SIZE_FILE"
printf '%s\n' $((2 * 1024 * 1024)) > "$DYLINT_SIZE_FILE"
mkdir -p "$project/target/wasm32-unknown-unknown/debug" "$project/target/dylint"
expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" --lane wasm-rebuild \
  --preserve-cache-lane wasm-debug -- sh -c 'test -d target/wasm32-unknown-unknown/debug && test ! -d target/dylint'
[[ -d "$project/target/wasm32-unknown-unknown/debug" ]]
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e 'all(.cleanup_events[]; .lane!="wasm-debug" or .outcome!="removed") and
  any(.cleanup_events[]; .lane=="dylint" and .outcome=="removed") and
  .cleanup.target_above_soft_cap==true and
  any(.cleanup_events[]; .operation=="guarded-lane")' "$receipt" >/dev/null
reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((5 * 1024 * 1024)) > "$WASM_SIZE_FILE"
printf '%s\n' $((14 * 1024 * 1024)) > "$FREE_FILE"
mkdir -p "$project/target/wasm32-unknown-unknown/debug"
touch "$PREVIEW_INCOMING_FILE"
expect_status 1 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --preserve-cache-lane wasm-debug -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" && -d "$project/target/wasm32-unknown-unknown/debug" ]]
! rg -v -- '--dry-run' "$CALLS_FILE" | rg -q .
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e 'any(.cleanup_events[]; .operation=="sweep" and .reason=="protected-incoming-lane")' "$receipt" >/dev/null

# A direct debug wasm Cargo invocation identifies its own incoming cache.
reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((5 * 1024 * 1024)) > "$WASM_SIZE_FILE"
mkdir -p "$project/target/wasm32-unknown-unknown/debug"
expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" --lane wasm-rebuild -- \
  cargo test --target wasm32-unknown-unknown
[[ -d "$project/target/wasm32-unknown-unknown/debug" ]]
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e 'all(.cleanup_events[]; .lane!="wasm-debug" or .outcome!="removed")' "$receipt" >/dev/null

# The soft cap may still be exceeded by useful debug dependencies after all
# independent inactive wasm, dylint and trybuild caches have been reclaimed.
reset_case
printf '%s\n' $((17 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((3 * 1024 * 1024)) > "$WASM_SIZE_FILE"
printf '%s\n' $((2 * 1024 * 1024)) > "$DYLINT_SIZE_FILE"
printf '%s\n' $((2 * 1024 * 1024)) > "$TRYBUILD_SIZE_FILE"
mkdir -p "$project/target/wasm32-unknown-unknown/debug" "$project/target/dylint" "$project/target/tests/trybuild" "$project/target/debug/deps"
printf 'reusable\n' > "$project/target/debug/deps/reusable.rlib"
expect_status 0 run_budget -- sh -c 'test -f target/debug/deps/reusable.rlib && test ! -d target/wasm32-unknown-unknown/debug && test ! -d target/dylint && test ! -d target/tests/trybuild'
[[ ! -s "$CALLS_FILE" && -f "$project/target/debug/deps/reusable.rlib" ]]
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.cleanup.target_above_soft_cap==true and
  ([.cleanup_events[] | select(.operation=="guarded-lane" and .outcome=="removed") | .lane][0]=="wasm-debug") and
  all(.cleanup_events[]; .operation!="sweep") and
  ([(.cleanup_events[] | select(.stage=="pre-admission" and .outcome=="removed") | .lane)] |
    index("wasm-debug")!=null and index("dylint")!=null and index("trybuild")!=null)' "$receipt" >/dev/null

# When the hard disk floor is low, try guarded inactive lanes first and only
# then allow the dependency sweep. The fake df stays low, so admission refuses.
reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((2 * 1024 * 1024)) > "$WASM_SIZE_FILE"
printf '%s\n' $((14 * 1024 * 1024)) > "$FREE_FILE"
mkdir -p "$project/target/wasm32-unknown-unknown/debug"
expect_status 1 run_budget -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" ]]
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '(.cleanup_events[0] | .stage=="pre-admission" and .operation=="guarded-lane" and .lane=="wasm-debug" and .outcome=="removed") and
  any(.cleanup_events[1:][]; .stage=="pre-admission" and .operation=="sweep" and .outcome=="completed") and
  .final_exit_status==1 and .current_phase=="finished" and .finished_utc!=null and
  .command_exit_status==null and .child==null' "$receipt" >/dev/null
[[ ! -d "${receipt}.write-lock" ]]

# Explicit incremental opt-in and loaded incremental files forbid preferential
# reclamation; existing scoped guards remain authoritative.
for protection in opt-in open-file; do
  reset_case
  printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
  mkdir -p "$project/target/debug/incremental"
  incremental=1
  if [[ "$protection" == open-file ]]; then
    incremental=0
    printf 'fake 123 %s\n' "$project/target/debug/incremental/loaded" > "$OPEN_TARGET_FILE"
  fi
  expect_status 0 env CARGO_INCREMENTAL="$incremental" bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" -- \
    sh -c 'test -d target/debug/incremental'
  if [[ "$protection" == open-file ]]; then
    receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
    jq -e 'any(.cleanup_events[];.operation=="guarded-lane" and .reason=="open-files" and
      .outcome=="refused" and .exit_status==1 and .reclaimed_kib==null) and
      all(.cleanup_events[];.operation!="sweep")' "$receipt" >/dev/null
  fi
done

reset_case
printf '%s\n' $((14 * 1024 * 1024)) > "$FREE_FILE"
expect_status 1 run_budget -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" ]]
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

reset_case
touch "$FAIL_SWEEP_FILE"
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
printf '%s\n' $((14 * 1024 * 1024)) > "$FREE_FILE"
expect_status 1 run_budget -- sh -c 'exit 0'
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

reset_case
printf 'cargo\n' > "$ACTIVE_FILE"
expect_status 1 run_budget -- sh -c 'exit 0'
[[ ! -s "$CALLS_FILE" ]]
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

reset_case
printf 'tool_repl\n' > "$ACTIVE_FILE"
expect_status 1 run_budget -- sh -c 'exit 0'
[[ ! -s "$CALLS_FILE" ]]

reset_case
printf 'tool_repl\n' > "$ACTIVE_FILE"
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
mkdir -p "$project/target/release" "$project/target/wasm32-unknown-unknown/debug"
printf 'keep\n' > "$project/target/release/production"
printf 'stale\n' > "$project/target/wasm32-unknown-unknown/debug/cache"
expect_status 0 run_budget --allow-live-harness -- sh -c 'exit 0'
if rg -v -- '--dry-run' "$CALLS_FILE" | rg -q .; then
  echo 'live harness mode performed a global sweep' >&2
  exit 1
fi
[[ -f "$project/target/release/production" ]]
[[ ! -e "$project/target/wasm32-unknown-unknown/debug" ]]

reset_case
mkdir "$project/target/.aura-build-budget.lock"
expect_status 1 run_budget -- sh -c 'exit 0'
[[ -d "$project/target/.aura-build-budget.lock" ]]

reset_case
expect_status 7 run_budget -- sh -c 'exit 7'
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.final_exit_status==7 and .command_exit_status==7 and .cleanup.result=="not-run-command-failed"' "$receipt" >/dev/null
[[ ! -s "$CALLS_FILE" ]]
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

reset_case
export CHILD_PID_FILE="$test_root/child.pid"
expect_status 75 run_budget -- sh -c 'echo $$ > "$CHILD_PID_FILE"; echo 4194304 > "$FREE_FILE"; exec sleep 20'
[[ -f "$CHILD_PID_FILE" ]]
if kill -0 "$(cat "$CHILD_PID_FILE")" 2>/dev/null; then
  echo 'emergency-floor child survived' >&2
  exit 1
fi
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

reset_case
rm -f "$CHILD_PID_FILE"
bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" --lane test -- \
  sh -c 'echo $$ > "$CHILD_PID_FILE"; exec sleep 20' > "$test_root/interrupt-output" 2>&1 &
wrapper_pid=$!
for _ in {1..30}; do
  [[ -f "$CHILD_PID_FILE" ]] && break
  sleep 0.1
done
[[ -f "$CHILD_PID_FILE" ]]
kill -TERM "$wrapper_pid"
if wait "$wrapper_pid"; then interrupt_status=0; else interrupt_status=$?; fi
[[ "$interrupt_status" -eq 143 ]]
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/interrupt-output")"
jq -e '.final_exit_status==143 and .command_exit_status==null and .current_phase=="finished"' "$receipt" >/dev/null
if kill -0 "$(cat "$CHILD_PID_FILE")" 2>/dev/null; then
  echo 'interrupted child survived' >&2
  exit 1
fi
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

# A sibling worktree's builder does not block this checkout.
reset_case
printf 'rustc\n' > "$ACTIVE_FILE"
expect_status 0 env ACTIVE_ROOT="$test_root/sibling" bash "$repo_root/scripts/dev/build-budget.sh" \
  --root "$project" --lane test -- sh -c 'exit 0'

# Cargo output goes to this checkout's target even if the shell named another.
reset_case
expect_status 0 env CARGO_TARGET_DIR=/elsewhere/target bash "$repo_root/scripts/dev/build-budget.sh" \
  --root "$project" --no-prune -- sh -c 'test -z "${CARGO_TARGET_DIR:-}"'

# A live reservation of another admitted build counts against the volume
# floor (20 GiB free - 6 GiB reserved < 15 GiB); a dead one is reclaimed.
reset_case
mkdir -p "$AURA_BUILD_SHARED_DIR/reservations"
sleep 30 &
holder_pid=$!
printf '%s\n' $((6 * 1024 * 1024)) > "$AURA_BUILD_SHARED_DIR/reservations/$holder_pid"
expect_status 1 run_budget --no-prune -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" ]]
rg -q 'other admitted builds' "$test_root/output"
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.final_exit_status==1 and .current_phase=="finished" and
  .finished_utc!=null and .command_exit_status==null and .child==null and
  .admission.admitted==false and .admission.refusal.reason=="insufficient-headroom" and
  .admission.refusal_count==1 and
  .failure_excerpt=="build admission refused; details in receipt"' "$receipt" >/dev/null
[[ ! -d "${receipt}.write-lock" ]]
kill "$holder_pid"; wait "$holder_pid" 2>/dev/null || true
expect_status 0 run_budget --no-prune -- sh -c 'ls "$AURA_BUILD_SHARED_DIR/reservations" > "$FREE_FILE.during"'
[[ "$(wc -l < "$FREE_FILE.during")" -eq 1 ]] # Only this build's own reservation.
[[ -z "$(ls -A "$AURA_BUILD_SHARED_DIR/reservations")" ]] # Released on exit.
[[ ! -d "$AURA_BUILD_SHARED_DIR/admission.lock" ]]

# A prior same-lane receipt reserves its observed peak growth for this incoming
# build. Both free-space drawdown and target growth count; a different lane or
# malformed receipt cannot change the estimate. Admission must refuse before
# the command runs and release its lock/reservation on failure.
reset_case
mkdir -p "$project/artifacts/disk-budget/receipts/run.prior"
cat > "$project/artifacts/disk-budget/receipts/run.prior/receipt.json" <<EOF
{"root":"$project","lane":"test","command_exit_status":0,"disk":{"measured_growth_kib":5242880}}
EOF
expect_status 1 run_budget --no-prune -- sh -c 'touch "$FREE_FILE.growth-ran"'
[[ ! -e "$FREE_FILE.growth-ran" ]]
rg -q 'incoming-reservation=6291456 KiB' "$test_root/output"
[[ -z "$(ls -A "$AURA_BUILD_SHARED_DIR/reservations")" ]]
printf '%s\n' $((22 * 1024 * 1024)) > "$FREE_FILE"
expect_status 0 run_budget --no-prune -- sh -c 'set -- "$AURA_BUILD_SHARED_DIR"/reservations/*; test "$#" -eq 1 && test "$(cat "$1")" = 6291456'
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.incoming_reservation_kib==6291456 and .disk.measured_growth_kib==0' "$receipt" >/dev/null

# Measured 11.3 GiB growth plus two active 9/10 GiB worktrees cannot fit a
# nominal 35 GiB free volume while preserving the 15 GiB floor. The command
# must not start or evict either hot worktree just to satisfy admission.
reset_case
printf '%s\n' $((35 * 1024 * 1024)) > "$FREE_FILE"
mkdir -p "$project/artifacts/disk-budget/receipts/run.hot" "$AURA_BUILD_SHARED_DIR/reservations"
printf '{"root":"%s","lane":"test","command_exit_status":0,"disk":{"measured_growth_kib":11848909}}\n' \
  "$project" > "$project/artifacts/disk-budget/receipts/run.hot/receipt.json"
sleep 30 & hot_a=$!
sleep 30 & hot_b=$!
printf '%s\n' $((9 * 1024 * 1024)) > "$AURA_BUILD_SHARED_DIR/reservations/$hot_a"
printf '%s\n' $((10 * 1024 * 1024)) > "$AURA_BUILD_SHARED_DIR/reservations/$hot_b"
expect_status 1 run_budget --no-prune -- sh -c 'touch "$FREE_FILE.hot-ran"'
[[ ! -e "$FREE_FILE.hot-ran" ]]
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.refusal.reason=="insufficient-headroom" and
  .admission.refusal.other_reservations_kib==19922944 and
  .admission.refusal.incoming_reservation_kib>=11848909 and
  .final_exit_status==1' "$receipt" >/dev/null
kill "$hot_a" "$hot_b"
wait "$hot_a" "$hot_b" 2>/dev/null || true

reset_case
mkdir -p "$project/artifacts/disk-budget/receipts/run.foreign" "$project/artifacts/disk-budget/receipts/run.bad"
printf '{"root":"%s","lane":"foreign","command_exit_status":0,"disk":{"measured_growth_kib":10485760}}\n' \
  "$project" > "$project/artifacts/disk-budget/receipts/run.foreign/receipt.json"
printf 'corrupt receipt\n' > "$project/artifacts/disk-budget/receipts/run.bad/receipt.json"
expect_status 0 run_budget --no-prune -- sh -c 'set -- "$AURA_BUILD_SHARED_DIR"/reservations/*; test "$#" -eq 1 && test "$(cat "$1")" = 4194304'

# Legacy TSV migration also works with BSD awk: -v assignments are single
# arguments and a different lane with the same native profile can supply a
# recent peak until the first structured receipt for this lane is available.
reset_case
mkdir -p "$project/artifacts/disk-budget"
host_triple="$(rustc -vV | awk '/^host: / {print $2}')"
printf 'start\tfinish\tlane\tprofile\ttriple\tfeatures\tcap\tmin\temergency\tbefore_free\tbefore_target\tpre_free\tpre_target\tlowest_free\tpeak_target\tafter_free\tafter_target\tstatus\n' \
  > "$project/artifacts/disk-budget/builds.tsv"
printf 'start\tfinish\tother-native\tunlabelled\t%s\tunlabelled\t8\t15\t5\t20971520\t6291456\t20971520\t6291456\t18874368\t6291456\t18874368\t6291456\t0\n' \
  "$host_triple" >> "$project/artifacts/disk-budget/builds.tsv"
expect_status 0 run_budget --no-prune -- sh -c 'set -- "$AURA_BUILD_SHARED_DIR"/reservations/*; test "$#" -eq 1 && test "$(cat "$1")" = 3145728'
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.incoming_reservation_kib==3145728' "$receipt" >/dev/null

# New policy execution lanes must not inherit a legacy profile/triple peak
# from unrelated compilation. They start with the existing conservative 4 GiB
# fallback, then their own measured receipt plus the 1 GiB sampling margin.
expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane policy-static-exec --no-prune -- \
  sh -c 'set -- "$AURA_BUILD_SHARED_DIR"/reservations/*; test "$(cat "$1")" = 4194304'
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.incoming_reservation_kib==4194304' "$receipt" >/dev/null
mkdir -p "$project/artifacts/disk-budget/receipts/run.policy-peak"
printf '{"root":"%s","lane":"policy-static-exec","command_exit_status":0,"disk":{"measured_growth_kib":524288}}\n' \
  "$project" > "$project/artifacts/disk-budget/receipts/run.policy-peak/receipt.json"
expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane policy-static-exec --no-prune -- \
  sh -c 'set -- "$AURA_BUILD_SHARED_DIR"/reservations/*; test "$(cat "$1")" = 1572864'
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.incoming_reservation_kib==1572864' "$receipt" >/dev/null
mkdir -p "$project/artifacts/disk-budget/receipts/run.failed-before-link"
printf '{"root":"%s","lane":"test","command_exit_status":101,"disk":{"measured_growth_kib":31748}}\n' \
  "$project" > "$project/artifacts/disk-budget/receipts/run.failed-before-link/receipt.json"
expect_status 0 run_budget --no-prune -- sh -c 'set -- "$AURA_BUILD_SHARED_DIR"/reservations/*; test "$#" -eq 1 && test "$(cat "$1")" = 3145728'
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.incoming_reservation_kib==3145728' "$receipt" >/dev/null

# Task 198: a gate runs under one budgeted hold; a nested budgeted call for
# the same checkout runs its command inside that hold (no second lock).
reset_case
nested="bash $repo_root/scripts/dev/build-budget.sh --root $project --lane nested -- sh -c 'touch \"\$FREE_FILE.nested-ran\"'"
expect_status 0 run_budget --no-prune -- sh -c "test -d target/.aura-build-budget.lock && $nested"
[[ -e "$FREE_FILE.nested-ran" ]]
rg -q 'runs inside the held budget' "$test_root/output"
# The hold is per checkout: another checkout's marker does not bypass the lock.
reset_case
mkdir "$project/target/.aura-build-budget.lock"
expect_status 1 env AURA_BUILD_BUDGET_HELD="$test_root/other" bash "$repo_root/scripts/dev/build-budget.sh" \
  --root "$project" --no-prune -- sh -c 'touch "$FREE_FILE.foreign-ran"'
[[ ! -e "$FREE_FILE.foreign-ran" ]]
# Gates wait for a busy lock up to AURA_BUILD_WAIT_SECONDS instead of failing.
reset_case
mkdir "$project/target/.aura-build-budget.lock"
(sleep 2; rm -rf "$project/target/.aura-build-budget.lock") &
expect_status 0 env AURA_BUILD_POLL_SECONDS=1 AURA_BUILD_WAIT_SECONDS=10 bash "$repo_root/scripts/dev/build-budget.sh" \
  --root "$project" --no-prune -- sh -c 'touch "$FREE_FILE.waited-ran"'
[[ -e "$FREE_FILE.waited-ran" ]]
wait
# Gates also wait out volume admission: another build's live reservation
# (20 - 6 < 15 GiB) blocks until that build exits.
reset_case
mkdir -p "$AURA_BUILD_SHARED_DIR/reservations"
sleep 3 &
holder_pid=$!
printf '%s\n' $((6 * 1024 * 1024)) > "$AURA_BUILD_SHARED_DIR/reservations/$holder_pid"
expect_status 0 env AURA_BUILD_POLL_SECONDS=1 AURA_BUILD_WAIT_SECONDS=20 bash "$repo_root/scripts/dev/build-budget.sh" \
  --root "$project" --no-prune -- sh -c 'touch "$FREE_FILE.admitted-ran"'
[[ -e "$FREE_FILE.admitted-ran" ]]
wait "$holder_pid" 2>/dev/null || true
# Task 209: a transient builder in this checkout (e.g. rust-analyzer's rustc)
# is waited out under AURA_BUILD_WAIT_SECONDS, both before the build and in
# the over-cap sweep; without a wait the build is refused at once.
reset_case
printf 'rustc\n' > "$ACTIVE_FILE"
expect_status 1 run_budget --no-prune -- sh -c 'touch "$FREE_FILE.busy-ran"'
[[ ! -e "$FREE_FILE.busy-ran" ]]
rg -q 'another builder or harness consumer is active' "$test_root/output"
for size_gib in 6 9; do
  reset_case
  printf '%s\n' $((size_gib * 1024 * 1024)) > "$SIZE_FILE"
  printf 'rustc\n' > "$ACTIVE_FILE"
  (sleep 2; rm -f "$ACTIVE_FILE") &
  expect_status 0 env AURA_BUILD_POLL_SECONDS=1 AURA_BUILD_WAIT_SECONDS=10 bash "$repo_root/scripts/dev/build-budget.sh" \
    --root "$project" --lane test -- sh -c 'touch "$FREE_FILE.idle-ran"'
  [[ -e "$FREE_FILE.idle-ran" ]]
  rm -f "$FREE_FILE.idle-ran"
  wait
done
# Task 212: a post-build sweep skipped because another builder is active,
# whether it started before the sweep or during its preview, keeps the
# build's own exit status (it was 76).
reset_case
expect_status 0 run_budget -- sh -c 'printf "rustc\n" > "$ACTIVE_FILE"; printf "%s\n" $((14 * 1024 * 1024)) > "$FREE_FILE"'
rg -q 'post-build sweep skipped because another builder started' "$test_root/output"
reset_case
touch "$RACE_ON_PREVIEW_FILE"
expect_status 0 run_budget -- sh -c 'printf "%s\n" $((14 * 1024 * 1024)) > "$FREE_FILE"'
rg -q 'post-build sweep skipped because another builder started' "$test_root/output"
reset_case
expect_status 3 run_budget -- sh -c 'printf "rustc\n" > "$ACTIVE_FILE"; exit 3'
# Task 216: a checkout lock left by a holder killed before its EXIT trap is
# reclaimed (logged) without waiting; a lock held by a live pid, or one with
# no pid yet, still blocks.
reset_case
sh -c 'exit 0' &
dead_pid=$!
wait "$dead_pid" 2>/dev/null || true
mkdir "$project/target/.aura-build-budget.lock"
printf '%s\n' "$dead_pid" > "$project/target/.aura-build-budget.lock/pid"
expect_status 0 run_budget --no-prune -- sh -c 'touch "$FREE_FILE.reclaimed-ran"'
[[ -e "$FREE_FILE.reclaimed-ran" ]]
rg -q "reclaiming .* from dead holder pid $dead_pid" "$test_root/output"
reset_case
sleep 30 &
live_pid=$!
mkdir "$project/target/.aura-build-budget.lock"
printf '%s\n' "$live_pid" > "$project/target/.aura-build-budget.lock/pid"
expect_status 1 run_budget --no-prune -- sh -c 'touch "$FREE_FILE.live-ran"'
[[ ! -e "$FREE_FILE.live-ran" ]]
kill "$live_pid"; wait "$live_pid" 2>/dev/null || true
reset_case
mkdir "$project/target/.aura-build-budget.lock"
expect_status 1 run_budget --no-prune -- sh -c 'touch "$FREE_FILE.nopid-ran"'
[[ ! -e "$FREE_FILE.nopid-ran" ]]
# Task 218: a build waiting for volume admission releases the checkout lock,
# so a prune (which needs that lock) can free space; the build then admits.
reset_case
printf '%s\n' $((14 * 1024 * 1024)) > "$FREE_FILE"
(
  for _ in $(seq 1 150); do
    if mkdir "$project/target/.aura-build-budget.lock" 2>/dev/null; then
      printf '%s\n' $((20 * 1024 * 1024)) > "$FREE_FILE"
      touch "$FREE_FILE.pruned"
      rmdir "$project/target/.aura-build-budget.lock"
      exit 0
    fi
    sleep 0.2
  done
  exit 1
) &
pruner=$!
expect_status 0 env AURA_BUILD_POLL_SECONDS=1 AURA_BUILD_WAIT_SECONDS=30 bash "$repo_root/scripts/dev/build-budget.sh" \
  --root "$project" --lane test -- sh -c 'touch "$FREE_FILE.admitted-after-prune"'
wait "$pruner"
[[ -e "$FREE_FILE.pruned" && -e "$FREE_FILE.admitted-after-prune" ]]
[[ ! -d "$project/target/.aura-build-budget.lock" ]]
# The gate recipes' cargo goes through the budget.
for recipe in _policy-check _ownership-lint web-check; do
  awk -v r="$recipe" '$0 ~ "^"r"[ :]" {on=1; next} on && /^[^ \t]/ {on=0} on' "$repo_root/justfile" \
    | rg -q 'build-budget.sh --lane gates' || { echo "justfile $recipe bypasses build-budget" >&2; exit 1; }
done

# The canonical user-flow gate compiles macros, but must not inherit broad
# ownership-gate peaks merely because both use lane=gates. Bind its recipe and
# selectors to an exact workload identity, retaining a 4 GiB cold fallback.
reset_case
printf 'ci-user-flow-policy:\n    @true\n' > "$project/justfile"
printf 'target/\nartifacts/\n' > "$project/.gitignore"
mkdir -p "$project/toolkit/xtask" "$project/crates/aura-macros"
for relative in Cargo.lock flake.nix flake.lock toolkit/xtask/Cargo.toml \
  toolkit/xtask/Cargo.lock crates/aura-macros/Cargo.toml; do
  printf 'fixture %s\n' "$relative" > "$project/$relative"
done
git -C "$project" init -q
git -C "$project" add Cargo.toml Cargo.lock flake.nix flake.lock justfile .gitignore \
  toolkit/xtask/Cargo.toml toolkit/xtask/Cargo.lock crates/aura-macros/Cargo.toml
git -C "$project" -c user.name=fixture -c user.email=fixture@example.invalid commit -qm workload-fixture
original_base="$(git -C "$project" rev-parse HEAD)"
git -C "$project" -c user.name=fixture -c user.email=fixture@example.invalid commit --allow-empty -qm intermediate-base
intermediate_base="$(git -C "$project" rev-parse HEAD)"
git -C "$project" -c user.name=fixture -c user.email=fixture@example.invalid commit --allow-empty -qm fixed-source
git -C "$project" update-ref refs/remotes/origin/main "$original_base"
cat > "$fakebin/just" <<'EOF'
#!/usr/bin/env bash
[[ "$1" == ci-user-flow-policy ]] || exit 2
EOF
chmod +x "$fakebin/just"
mkdir -p "$project/artifacts/disk-budget/receipts/run.broad"
printf '{"root":"%s","lane":"gates","command_exit_status":0,"disk":{"measured_growth_kib":10485760}}\n' \
  "$project" > "$project/artifacts/disk-budget/receipts/run.broad/receipt.json"
printf 'start\tfinish\tlane\tprofile\ttriple\tfeatures\tcap\tmin\temergency\tbefore_free\tbefore_target\tpre_free\tpre_target\tlowest_free\tpeak_target\tafter_free\tafter_target\tstatus\n' \
  > "$project/artifacts/disk-budget/builds.tsv"
printf 'start\tfinish\tgates\tunlabelled\t%s\tunlabelled\t8\t15\t5\t20971520\t6291456\t20971520\t6291456\t10485760\t6291456\t10485760\t6291456\t0\n' \
  "$host_triple" >> "$project/artifacts/disk-budget/builds.tsv"
expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane gates --no-prune -- just ci-user-flow-policy
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.incoming_reservation_kib==4194304 and
  (.admission.workload.identity|type)=="string" and .final_exit_status==0' "$receipt" >/dev/null
canonical_key="$(jq -r .admission.workload.identity "$receipt")"
canonical_source="$(jq -r .source.fingerprint "$receipt")"
generic_tree_before="$(find "$project/artifacts" "$project/target" "$AURA_BUILD_SHARED_DIR" -print0 | sort -z | sha256sum)"
generic_files_before="$(find "$project/artifacts" "$project/target" "$AURA_BUILD_SHARED_DIR" -type f -exec sha256sum '{}' + | LC_ALL=C sort | sha256sum)"
expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane gates --dry-run --no-prune -- just ci-ownership-policy
rg -q 'incoming-reservation=11534336 KiB.*outcome=refused identity=generic workload=generic' "$test_root/output"
[[ "$generic_tree_before" == "$(find "$project/artifacts" "$project/target" "$AURA_BUILD_SHARED_DIR" -print0 | sort -z | sha256sum)" &&
   "$generic_files_before" == "$(find "$project/artifacts" "$project/target" "$AURA_BUILD_SHARED_DIR" -type f -exec sha256sum '{}' + | LC_ALL=C sort | sha256sum)" ]] || {
  echo 'generic preview changed protected artifact tree' >&2; exit 1;
}
mkdir -p "$project/artifacts/disk-budget/receipts/run.same"
jq -n --arg root "$project" --arg identity "$canonical_key" \
  '{root:$root,lane:"gates",admission:{workload:{identity:$identity}},
    command_exit_status:0,disk:{measured_growth_kib:2097152}}' \
  > "$project/artifacts/disk-budget/receipts/run.same/receipt.json"
expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane gates --no-prune -- just ci-user-flow-policy
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e --arg identity "$canonical_key" '.admission.incoming_reservation_kib==3145728 and
  .admission.workload.identity==$identity' "$receipt" >/dev/null
preview_tree_before="$(find "$project/artifacts" "$project/target" "$AURA_BUILD_SHARED_DIR" -print0 | sort -z | sha256sum)"
preview_files_before="$(find "$project/artifacts" "$project/target" "$AURA_BUILD_SHARED_DIR" -type f -exec sha256sum '{}' + | LC_ALL=C sort | sha256sum)"
touch -t 202001020304 "$project/justfile" # Same bytes, changed mtime must not refresh .git/index.
preview_git_index_before="$(sha256sum "$project/.git/index")"
expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane gates --dry-run --no-prune -- just ci-user-flow-policy
rg -q "incoming-reservation=3145728 KiB.*workload=$canonical_key" "$test_root/output"
rg -q 'outcome=admissible identity=certified' "$test_root/output"
[[ "$preview_tree_before" == "$(find "$project/artifacts" "$project/target" "$AURA_BUILD_SHARED_DIR" -print0 | sort -z | sha256sum)" &&
   "$preview_files_before" == "$(find "$project/artifacts" "$project/target" "$AURA_BUILD_SHARED_DIR" -type f -exec sha256sum '{}' + | LC_ALL=C sort | sha256sum)" ]] || {
  echo 'certified preview changed protected artifact tree' >&2; exit 1;
}
[[ "$preview_git_index_before" == "$(sha256sum "$project/.git/index")" ]] || {
  echo 'certified preview changed Git index' >&2; exit 1;
}
[[ ! -e "$FREE_FILE.preview-command-ran" ]]
# Advancing the default origin base changes policy inputs without changing
# HEAD or any checkout source. Do not reuse the old measured reservation.
git -C "$project" update-ref refs/remotes/origin/main "$intermediate_base"
expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane gates --no-prune -- just ci-user-flow-policy
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e --arg old "$canonical_key" --arg source "$canonical_source" \
  '.source.fingerprint==$source and .admission.workload.identity!=$old and
   .admission.incoming_reservation_kib==4194304' "$receipt" >/dev/null
git -C "$project" update-ref refs/remotes/origin/main "$original_base"
cp "$fakebin/just" "$test_root/just-original"
for changed in recipe features profile target lock tool config policy-range guidance-range github-base guidance-files guidance-empty; do
  case "$changed" in
    recipe) printf 'ci-user-flow-policy:\n    @true # revised\n' > "$project/justfile" ;;
    features) export AURA_BUILD_FEATURES=alternate ;;
    profile) export AURA_BUILD_PROFILE=alternate ;;
    target) export AURA_BUILD_TARGET_TRIPLE=alternate-target ;;
    lock) printf 'changed lock\n' > "$project/Cargo.lock" ;;
    tool) printf '# changed tool\n' >> "$fakebin/just" ;;
    config) mkdir -p "$project/.cargo"; printf 'changed config\n' > "$project/.cargo/config.toml" ;;
    policy-range) export AURA_UX_POLICY_DIFF_RANGE=alternate-policy ;;
    guidance-range) export AURA_UX_GUIDANCE_DIFF_RANGE=alternate-guidance ;;
    github-base) export GITHUB_BASE_REF=alternate-base ;;
    guidance-files) export AURA_UX_GUIDANCE_CHANGED_FILES=crates/alternate.rs ;;
    guidance-empty) export AURA_UX_GUIDANCE_CHANGED_FILES='' ;;
  esac
  expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
    --lane gates --no-prune -- just ci-user-flow-policy
  receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
  jq -e --arg old "$canonical_key" '.admission.incoming_reservation_kib==4194304 and
    .admission.workload.identity!=$old' "$receipt" >/dev/null
  case "$changed" in
    recipe) printf 'ci-user-flow-policy:\n    @true\n' > "$project/justfile" ;;
    features) unset AURA_BUILD_FEATURES ;;
    profile) unset AURA_BUILD_PROFILE ;;
    target) unset AURA_BUILD_TARGET_TRIPLE ;;
    lock) printf 'fixture Cargo.lock\n' > "$project/Cargo.lock" ;;
    tool) cp "$test_root/just-original" "$fakebin/just"; chmod +x "$fakebin/just" ;;
    config) rm "$project/.cargo/config.toml"; rmdir "$project/.cargo" ;;
    policy-range) unset AURA_UX_POLICY_DIFF_RANGE ;;
    guidance-range) unset AURA_UX_GUIDANCE_DIFF_RANGE ;;
    github-base) unset GITHUB_BASE_REF ;;
    guidance-files|guidance-empty) unset AURA_UX_GUIDANCE_CHANGED_FILES ;;
  esac
done
expect_status 0 env RUSTC=/missing/aura-preview-compiler \
  bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
    --lane gates --dry-run --no-prune -- just ci-user-flow-policy
rg -q 'canonical workload identity unavailable' "$test_root/output"
rg -q 'outcome=unknown identity=uncertified-canonical workload=generic' "$test_root/output"
graft_path="$(git -C "$project" rev-parse --path-format=absolute --git-path info/grafts)"
printf '\n' > "$graft_path"
expect_status 1 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane gates --no-prune -- just ci-user-flow-policy
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.workload==null and
  .admission.refusal.incoming_reservation_kib==11534336' "$receipt" >/dev/null
rm "$graft_path"
# A shallow history is not certified by a ref map alone.
printf '%s\n' "$original_base" > "$project/.git/shallow"
expect_status 1 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane gates --no-prune -- just ci-user-flow-policy
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.workload==null and
  .admission.refusal.incoming_reservation_kib==11534336' "$receipt" >/dev/null
rm "$project/.git/shallow"
for override in JUST_JUSTFILE JUST_WORKING_DIRECTORY; do
  export "$override"=other-recipe
  expect_status 0 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
    --lane gates --dry-run --no-prune -- just ci-user-flow-policy
  rg -q 'canonical workload identity unavailable' "$test_root/output"
  rg -q 'incoming-reservation=11534336 KiB.*outcome=unknown identity=uncertified-canonical workload=generic' "$test_root/output"
  expect_status 1 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
    --lane gates --no-prune -- just ci-user-flow-policy
  receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
  jq -e '.admission.workload==null and
    .admission.refusal.incoming_reservation_kib==11534336' "$receipt" >/dev/null
  unset "$override"
done
mkdir -p "$project/artifacts/disk-budget/receipts/run.same-failed"
jq -n --arg root "$project" --arg identity "$canonical_key" \
  '{root:$root,lane:"gates",admission:{workload:{identity:$identity}},
    command_exit_status:101,disk:{measured_growth_kib:5242880}}' \
  > "$project/artifacts/disk-budget/receipts/run.same-failed/receipt.json"
expect_status 1 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane gates --no-prune -- just ci-user-flow-policy
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.refusal.incoming_reservation_kib==6291456 and
  .admission.admitted==false' "$receipt" >/dev/null
expect_status 1 bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
  --lane gates --no-prune -- just ci-user-flow-policy extra
receipt="$(sed -n 's/^Build receipt: //p' "$test_root/output")"
jq -e '.admission.workload==null and
  .admission.refusal.incoming_reservation_kib==11534336' "$receipt" >/dev/null

echo 'build-budget safety tests passed'
