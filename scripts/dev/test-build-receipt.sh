#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-build-receipt-test.XXXXXX")"
phase_pid=''
writer_pids=()
cleanup_fixture() {
  local writer child
  for writer in "${writer_pids[@]}"; do
    for child in $(ps -axo pid=,ppid= | awk -v parent="$writer" '$2==parent {print $1}'); do
      kill -TERM "$child" 2>/dev/null || true
    done
    kill -TERM "$writer" 2>/dev/null || true
    wait "$writer" 2>/dev/null || true
  done
  if [[ -n "$phase_pid" ]]; then kill -TERM "$phase_pid" 2>/dev/null || true; wait "$phase_pid" 2>/dev/null || true; fi
  rm -rf "$fixture"
}
trap cleanup_fixture EXIT
git -C "$fixture" init -q
printf '/artifacts/\n' > "$fixture/.gitignore"
printf 'original\n' > "$fixture/source.rs"
git -C "$fixture" add .gitignore source.rs
git -C "$fixture" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
source "$repo_root/scripts/dev/build-receipt.sh"
aura_receipt_init "$fixture" fixture cargo check --locked -p aura-simulator --features 'signals, test'
jq -e '.command_argv==["cargo","check","--locked","-p","aura-simulator","--features","signals, test"]' "$AURA_BUILD_RECEIPT" >/dev/null
original="$(jq -r '.source.fingerprint' "$AURA_BUILD_RECEIPT")"
printf 'new module\n' > "$fixture/new.rs"
aura_receipt_init "$fixture" fixture sh -c 'exit 0'
with_new="$(jq -r '.source.fingerprint' "$AURA_BUILD_RECEIPT")"
[[ "$original" != "$with_new" ]]
printf 'edited new module\n' > "$fixture/new.rs"
aura_receipt_init "$fixture" fixture sh -c 'exit 0'
[[ "$with_new" != "$(jq -r '.source.fingerprint' "$AURA_BUILD_RECEIPT")" ]]
printf 'edited tracked\n' > "$fixture/source.rs"
aura_receipt_init "$fixture" fixture sh -c 'exit 0'
jq -e '.metadata.packages==null and .metadata.features==null and .command_exit_status==null' "$AURA_BUILD_RECEIPT" >/dev/null
printf 'first\nsecond\n' > "$fixture/expected"
phase() { bash "$repo_root/scripts/dev/build-phase.sh" "$@"; }
phase compile -- sh -c 'test "$1" = --locked && test "$2" = -p && test "$3" = "package with spaces"' sh --locked -p 'package with spaces'
jq -e '.phases[0].command_argv==["sh","-c",
  "test \"$1\" = --locked && test \"$2\" = -p && test \"$3\" = \"package with spaces\"",
  "sh","--locked","-p","package with spaces"]' "$AURA_BUILD_RECEIPT" >/dev/null
phase discovery --expected-tests "$fixture/expected" -- printf 'second: test\nfirst: test\n'
phase execution --expected-tests "$fixture/expected" -- printf 'test first ... ok\ntest second ... ok\n'
status=0
phase execution --expected-tests "$fixture/expected" -- printf 'test first ... ok\n' || status=$?
[[ "$status" == 78 ]]
status=0
phase discovery --expected-tests "$fixture/expected" -- printf '0 tests, 0 benchmarks\n' || status=$?
[[ "$status" == 78 ]]
status=0
phase compile -- sh -c 'echo native-failure; exit 37' || status=$?
[[ "$status" == 37 ]]
# The acceptance inventory is selected before the command can mutate its file.
status=0
phase discovery --expected-tests "$fixture/expected" -- sh -c \
  'printf "first\n" > "$1"; printf "first: test\n"' sh "$fixture/expected" || status=$?
[[ "$status" == 78 ]]
# Observe the actual phase process while its command is held at an owned gate.
mkfifo "$fixture/release"
bash "$repo_root/scripts/dev/build-phase.sh" compile -- sh -c 'read -r release < "$1"' sh "$fixture/release" &
phase_pid=$!
observed=0
for attempt in $(seq 1 100); do
  if jq -e --argjson pid "$phase_pid" '.active_phase.phase=="compile" and
    .active_phase.runner.pid==$pid and .active_phase.child.pid!=null and .active_phase.child.start_identity!=null and
    .active_phase.runner.start_identity!=null' "$AURA_BUILD_RECEIPT" >/dev/null; then
    observed=1; break
  fi
  sleep 0.02
done
[[ "$observed" == 1 ]]
printf 'release\n' > "$fixture/release"
wait "$phase_pid"
phase_pid=''
# Disposal is recorded independently of the preceding semantic/compile failure.
phase disposal -- sh -c 'exit 0'
jq -e '(.phases|length)==9 and .phases[1].observed_count==2 and
  .phases[2].inventory_result=="matched" and .phases[3].command_exit_status==0 and
  .phases[3].exit_status==78 and .phases[4].observed_count==0 and
  .phases[5].command_exit_status==37 and .phases[6].exit_status==78 and
  .phases[7].child.start_identity!=null and .phases[8].exit_status==0 and
  .active_phase==null' "$AURA_BUILD_RECEIPT" >/dev/null
printf 'failed_fixture\n' > "$fixture/failed-expected"
status=0
phase execution --expected-tests "$fixture/failed-expected" -- sh -c \
  'printf "test failed_fixture ... FAILED\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n"; exit 101' || status=$?
[[ "$status" == 101 ]]
jq -e '.phases[-1] | .inventory_result=="matched" and .observed_count==1 and
  .observed_tests==["failed_fixture"] and .passed_count==0 and .failed_count==1 and
  .ignored_count==0 and .command_exit_status==101 and .exit_status==101' "$AURA_BUILD_RECEIPT" >/dev/null
status=0
phase execution --expected-tests "$fixture/failed-expected" -- printf \
  'test failed_fixture ... ignored, required fixture cannot be skipped\n' || status=$?
[[ "$status" == 78 ]]
jq -e '.phases[-1] | .inventory_result=="mismatch" and .observed_count==0 and
  .ignored_count==1 and .passed_count==0 and .failed_count==0 and
  .command_exit_status==0 and .exit_status==78' "$AURA_BUILD_RECEIPT" >/dev/null
# Hold one read/modify/write at a FIFO while a second native writer contends.
# Both phase and cleanup appends must survive; an atomic rename is insufficient.
mkfifo "$fixture/write-gate"
aura_receipt_update --slurpfile gate "$fixture/write-gate" \
  '.phases += [{phase:"overlap-fixture"}]' &
writer_pids+=("$!")
locked=0
for attempt in $(seq 1 100); do
  if [[ -f "${AURA_BUILD_RECEIPT}.write-lock/pid" ]]; then locked=1; break; fi
  sleep 0.02
done
[[ "$locked" == 1 ]]
mkdir -p "$fixture/contention-bin"
export RECEIPT_TEST_REAL_MKDIR="$(command -v mkdir)"
export RECEIPT_TEST_CONTENDED="$fixture/contended"
cat > "$fixture/contention-bin/mkdir" <<'MKDIR'
#!/usr/bin/env bash
status=0
"$RECEIPT_TEST_REAL_MKDIR" "$@" || status=$?
if [[ "$status" != 0 && "$1" == "${AURA_BUILD_RECEIPT}.write-lock" ]]; then
  printf 'actual lock contention\n' > "$RECEIPT_TEST_CONTENDED"
fi
exit "$status"
MKDIR
chmod +x "$fixture/contention-bin/mkdir"
PATH="$fixture/contention-bin:$PATH" aura_receipt_update '.cleanup_events += [{operation:"overlap-fixture"}]' &
writer_pids+=("$!")
for attempt in $(seq 1 100); do
  [[ ! -s "$RECEIPT_TEST_CONTENDED" ]] || break
  sleep 0.02
done
[[ -s "$RECEIPT_TEST_CONTENDED" ]]
printf '{}\n' > "$fixture/write-gate"
for writer in "${writer_pids[@]}"; do wait "$writer"; done
writer_pids=()
jq -e 'any(.phases[];.phase=="overlap-fixture") and
  any(.cleanup_events[];.operation=="overlap-fixture")' "$AURA_BUILD_RECEIPT" >/dev/null
[[ ! -d "${AURA_BUILD_RECEIPT}.write-lock" ]]
# A stopped writer's lock is retained rather than unsafely reclaimed.
sh -c 'exit 0' &
dead_writer=$!
wait "$dead_writer"
mkdir "${AURA_BUILD_RECEIPT}.write-lock"
printf '%s\n' "$dead_writer" > "${AURA_BUILD_RECEIPT}.write-lock/pid"
status=0
aura_receipt_update '.unexpected_reclaim=true' || status=$?
[[ "$status" == 79 && "$(cat "${AURA_BUILD_RECEIPT}.write-lock/pid")" == "$dead_writer" ]]
jq -e '.unexpected_reclaim==null' "$AURA_BUILD_RECEIPT" >/dev/null
# Explicit fixture teardown only; production never reclaims this lock.
rm "${AURA_BUILD_RECEIPT}.write-lock/pid"
rmdir "${AURA_BUILD_RECEIPT}.write-lock"
aura_receipt_finish 37
jq -e '.final_exit_status==37 and .current_phase=="finished" and .finished_utc!=null' "$AURA_BUILD_RECEIPT" >/dev/null
if phase compile -- sh -c 'exit 0'; then echo 'finished receipt was reused' >&2; exit 1; fi
echo 'build receipts: source fingerprint, unknown metadata, exact inventories, command failures and separate disposal passed'
