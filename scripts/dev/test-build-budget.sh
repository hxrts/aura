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
export FAIL_SWEEP_FILE="$test_root/fail-sweep"
export PROTECTED_FILE="$test_root/protected-release"
export PROTECTED_WEB_FILE="$test_root/protected-web"
export RACE_ON_PREVIEW_FILE="$test_root/race-on-preview"
export OPEN_TARGET_FILE="$test_root/open-target"
export DU_FAIL_ONCE_FILE="$test_root/du-fail-once"
export DU_EMPTY_FILE="$test_root/du-empty"
export AURA_BUILD_TARGET_CAP_GIB=8
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
printf '%s\t%s\n' "$(cat "$SIZE_FILE")" "${@: -1}"
if [[ -f "$DU_FAIL_ONCE_FILE" ]]; then
  rm -f "$DU_FAIL_ONCE_FILE"
  echo 'du: transient disappearing Cargo artifact' >&2
  exit 1
fi
EOF
cat > "$fakebin/ps" <<'EOF'
#!/usr/bin/env bash
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
if [[ -f "$RACE_ON_PREVIEW_FILE" && " $* " == *' --dry-run '* ]]; then
  printf 'cargo\n' > "$ACTIVE_FILE"
fi
[[ ! -f "$FAIL_SWEEP_FILE" ]]
EOF
chmod +x "$fakebin"/*

reset_case() {
  printf '%s\n' $((20 * 1024 * 1024)) > "$FREE_FILE"
  printf '%s\n' $((6 * 1024 * 1024)) > "$SIZE_FILE"
  : > "$CALLS_FILE"
  rm -f "$ACTIVE_FILE" "$FAIL_SWEEP_FILE" "$PROTECTED_FILE" "$PROTECTED_WEB_FILE" "$RACE_ON_PREVIEW_FILE" "$OPEN_TARGET_FILE" "$DU_FAIL_ONCE_FILE" "$DU_EMPTY_FILE"
  rm -rf "$project/target/.aura-build-budget.lock"
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
[[ "$(wc -l < "$CALLS_FILE")" -eq 2 ]] # Post-build preview, then apply.
[[ ! -d "$project/target/.aura-build-budget.lock" ]]
[[ "$(awk -F '\t' 'NR == 2 {print NF}' "$project/artifacts/disk-budget/builds.tsv")" -eq 18 ]]
rg -l 'Build context: lane=test' "$project"/artifacts/disk-budget/report.* >/dev/null

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
expect_status 1 run_budget --no-prune -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" && ! -s "$CALLS_FILE" ]]

reset_case
rmdir "$project/target"
ln -s "$test_root" "$project/target"
expect_status 1 run_budget -- sh -c 'exit 0'
rm "$project/target"
mkdir "$project/target"

reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
expect_status 0 run_budget -- sh -c 'exit 0'
[[ "$(wc -l < "$CALLS_FILE")" -eq 4 ]] # Pre- and post-build preview/apply.
rg -q -- '--dry-run' "$CALLS_FILE"

reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
touch "$RACE_ON_PREVIEW_FILE"
expect_status 1 run_budget -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" ]]
[[ "$(wc -l < "$CALLS_FILE")" -eq 1 ]]
rg -q -- '--dry-run' "$CALLS_FILE"
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
mkdir -p "$project/target/release" "$project/target/wasm32-unknown-unknown/debug"
printf 'loaded\n' > "$project/target/release/loaded"
printf 'stale\n' > "$project/target/wasm32-unknown-unknown/debug/cache"
printf 'fake 123 %s\n' "$project/target/release/loaded" > "$OPEN_TARGET_FILE"
expect_status 0 run_budget -- sh -c 'exit 0'
if rg -v -- '--dry-run' "$CALLS_FILE" | rg -q .; then
  echo 'open target file was swept' >&2
  exit 1
fi
[[ -f "$project/target/release/loaded" ]]
[[ ! -e "$project/target/wasm32-unknown-unknown/debug" ]]

reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
touch "$PROTECTED_FILE"
mkdir -p "$project/target/release" "$project/target/wasm32-unknown-unknown/debug"
printf 'keep\n' > "$project/target/release/production"
printf 'stale\n' > "$project/target/wasm32-unknown-unknown/debug/cache"
expect_status 0 run_budget -- sh -c 'exit 0'
if rg -v -- '--dry-run' "$CALLS_FILE" | rg -q .; then
  echo 'protected release cache was swept' >&2
  exit 1
fi
rg -q 'Protected production/web candidates: 1' "$test_root/output"
[[ -f "$project/target/release/production" ]]
[[ ! -e "$project/target/wasm32-unknown-unknown/debug" ]]

reset_case
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
touch "$PROTECTED_WEB_FILE"
expect_status 0 run_budget -- sh -c 'exit 0'
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

reset_case
printf '%s\n' $((14 * 1024 * 1024)) > "$FREE_FILE"
expect_status 1 run_budget -- sh -c 'touch "$FREE_FILE.build-ran"'
[[ ! -e "$FREE_FILE.build-ran" ]]
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

reset_case
touch "$FAIL_SWEEP_FILE"
printf '%s\n' $((11 * 1024 * 1024)) > "$SIZE_FILE"
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
  --root "$project" --no-prune -- sh -c 'test "$CARGO_TARGET_DIR" = "$PROJECT_ROOT/target"'

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
kill "$holder_pid"; wait "$holder_pid" 2>/dev/null || true
expect_status 0 run_budget --no-prune -- sh -c 'ls "$AURA_BUILD_SHARED_DIR/reservations" > "$FREE_FILE.during"'
[[ "$(wc -l < "$FREE_FILE.during")" -eq 1 ]] # Only this build's own reservation.
[[ -z "$(ls -A "$AURA_BUILD_SHARED_DIR/reservations")" ]] # Released on exit.
[[ ! -d "$AURA_BUILD_SHARED_DIR/admission.lock" ]]

echo 'build-budget safety tests passed'
