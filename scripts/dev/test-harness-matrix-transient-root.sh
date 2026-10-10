#!/usr/bin/env bash
set -euo pipefail
source_script="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/scripts/harness/run-matrix.sh}"
fixture="$(mktemp -d /tmp/ah-matrix-test.XXXXXXXX)"
trap 'rm -rf "$fixture"' EXIT
repo="$fixture/$(printf '%0120d' 0)"
mkdir -p "$repo/scripts/harness" "$repo/scenarios" "$repo/configs/harness"
cp "$source_script" "$repo/scripts/harness/run-matrix.sh"
cat > "$repo/scenarios/harness_inventory.toml" <<'EOF'
[[scenario]]
id = "owned-socket-root"
path = "scenarios/owned.toml"
classification = "shared"
EOF
printf 'mode = "local"\n' > "$repo/configs/harness/local-loopback.toml"
cat > "$fixture/fake-harness" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n%s\n%s\n' "$AURA_HARNESS_TRANSIENT_ROOT" "$AURA_HARNESS_RUN_ROOT" "$$" > "$ROOT_RECORD"
[[ -d "$AURA_HARNESS_TRANSIENT_ROOT" && -O "$AURA_HARNESS_TRANSIENT_ROOT" && ! -L "$AURA_HARNESS_TRANSIENT_ROOT" ]]
mkdir -p "$AURA_HARNESS_TRANSIENT_ROOT/actual-owned-scope"
case "$CASE_MODE" in
 success) exit 0 ;;
 failure) exit 7 ;;
 term)
  trap '[[ -d "$AURA_HARNESS_TRANSIENT_ROOT/actual-owned-scope" && -d "$AURA_HARNESS_RUN_ROOT" ]] && printf "roots-retained-before-exit\n" > "$ROOT_RECORD.settled"; exit 143' TERM
  printf "ready\n" > "$READY_FIFO"
  read -r _ < "$HOLD_FIFO"
  ;;
esac
EOF
chmod +x "$fixture/fake-harness"
mkdir -p "$fixture/bin" "$repo/scripts/dev" "$repo/target/debug"
cat > "$repo/scripts/dev/build-budget.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == --lane && "$2" == gates && "$3" == -- ]]
shift 3
exec "$@"
EOF
cat > "$fixture/bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == build ]]
printf 'prepared\n' >> "$BUILD_RECORD"
EOF
chmod +x "$repo/scripts/dev/build-budget.sh" "$fixture/bin/cargo"
cp "$fixture/fake-harness" "$repo/target/debug/aura-harness"
cp "$fixture/fake-harness" "$repo/target/debug/aura"
for mode in success failure keep; do
 record="$fixture/$mode.record"
 status=0
 actual_mode="$mode"
 keep=0
 if [[ "$mode" == keep ]]; then actual_mode=success; keep=1; fi
 ROOT_RECORD="$record" CASE_MODE="$actual_mode" AURA_HARNESS_KEEP_TMP="$keep" AURA_HARNESS_BIN="$fixture/fake-harness" AURA_HARNESS_AURA_BIN="$fixture/fake-harness" bash "$repo/scripts/harness/run-matrix.sh" --lane tui > "$fixture/$mode.log" 2>&1 || status=$?
 [[ "$actual_mode" != failure || "$status" -eq 7 ]]
 [[ "$actual_mode" != success || "$status" -eq 0 ]]
 transient="$(sed -n '1p' "$record")"
 run_root="$(sed -n '2p' "$record")"
 [[ "$transient" == /tmp/ah-matrix.* ]]
 [[ ${#transient} -lt 60 ]]
 if [[ "$mode" == keep ]]; then
  [[ -d "$transient/actual-owned-scope" && -d "$run_root" ]]
  rm -rf "$transient" "$run_root"
 else
  [[ ! -e "$transient" && ! -e "$run_root" ]]
 fi
 printf 'matrix private-root %s PASS\n' "$mode"
done
# A real external TERM must settle the original run scope, independently of
# whether its fake backend currently holds an unfinished read.
for launch in supplied prepared; do
record="$fixture/term-$launch.record"
rm -f "$fixture/ready" "$fixture/hold"
mkfifo "$fixture/ready" "$fixture/hold"
set -m
binary="$fixture/fake-harness"
if [[ "$launch" == prepared ]]; then binary=""; fi
PATH="$fixture/bin:$PATH" BUILD_RECORD="$fixture/build.record" ROOT_RECORD="$record" CASE_MODE=term READY_FIFO="$fixture/ready" HOLD_FIFO="$fixture/hold" AURA_HARNESS_KEEP_TMP=0 AURA_HARNESS_BIN="$binary" AURA_HARNESS_AURA_BIN="$binary" bash "$repo/scripts/harness/run-matrix.sh" --lane tui > "$fixture/term-$launch.log" 2>&1 &
matrix_pid=$!
read -r _ < "$fixture/ready"
kill -TERM "$matrix_pid"
status=0
wait "$matrix_pid" || status=$?
transient="$(sed -n '1p' "$record")"
run_root="$(sed -n '2p' "$record")"
backend_pid="$(sed -n '3p' "$record")"
[[ "$(cat "$record.settled")" == roots-retained-before-exit ]]
if kill -0 "$backend_pid" 2>/dev/null; then
 printf 'matrix returned before original held backend settled\n' >&2
 kill -TERM -- "-$matrix_pid" 2>/dev/null || true
 exit 1
fi
if [[ -e "$transient" || -e "$run_root" ]]; then
 printf 'matrix external TERM retained owned roots (status=%s)\n' "$status" >&2
 rm -rf "$transient" "$run_root"
 exit 1
fi
[[ "$status" -ne 0 ]]
[[ "$status" -eq 143 ]]
if [[ "$launch" == prepared ]]; then [[ "$(wc -l < "$fixture/build.record" | tr -d ' ')" -eq 2 ]]; fi
printf 'matrix private-root TERM %s PASS\n' "$launch"
done
