#!/usr/bin/env bash
# Exercise the tracked LAN driver lifecycle against an isolated fake tool_repl.
set -euo pipefail
# This fixture supplies its own tools and never enters the real Nix shell.
export IN_NIX_SHELL=fixture

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-lan-retention-test.XXXXXX")"
test_root="$(cd "$test_root" && pwd -P)"
driver="$repo_root/scripts/harness/lan/drv.sh"
export AURA_E2E_ROOT="$test_root"
export AURA_E2E_RUN_DIR="$test_root/.tmp/e2e/run"
export AURA_E2E_TOOL_REPL="$test_root/fake-tool-repl.sh"
export AURA_E2E_RUN_TOKEN=lan-retention-test-1
export AURA_LAN_FIXTURE_PROVIDER_REPORT="$test_root/provider-selection"
export AURA_LAN_FIXTURE_READY="$test_root/ready"
export AURA_E2E_HOST_ADDR=127.0.0.1
mkfifo "$AURA_LAN_FIXTURE_READY"
exec 9<>"$AURA_LAN_FIXTURE_READY"
mkdir -p "$test_root/scripts/dev" "$test_root/configs"
cp "$repo_root/scripts/dev/retain-e2e-runs.sh" "$test_root/scripts/dev/retain-e2e-runs.sh"
cat > "$AURA_E2E_TOOL_REPL" <<'EOF'
#!/usr/bin/env bash
[[ "${AURA_SECURE_STORAGE_BACKEND:-}" == filesystem-fallback ]] || exit 91
printf '%s\n' "$AURA_SECURE_STORAGE_BACKEND" > "$AURA_LAN_FIXTURE_PROVIDER_REPORT"
trap 'exit 0' TERM
printf 'ready\n' > "$AURA_LAN_FIXTURE_READY"
while IFS= read -r _; do :; done
EOF
chmod +x "$AURA_E2E_TOOL_REPL"
cat > "$test_root/configs/lan.toml" <<'EOF'
[run]
name = "test"
artifact_dir = ".tmp/e2e/run/host-a/artifacts"
EOF
lock_owner=''
cleanup() {
  if [[ -n "$lock_owner" ]]; then
    printf 'release\n' >&6 || true
    wait "$lock_owner" 2>/dev/null || true
  fi
  if bash "$driver" stop >/dev/null 2>&1; then
    rm -rf "$test_root"
  else
    echo "retaining fixture state with unresolved lifecycle ownership: $test_root" >&2
  fi
}
trap cleanup EXIT

bash "$driver" start "$test_root/configs/lan.toml" >/dev/null
read -r ready <&9
[[ "$ready" == ready ]]
[[ "$(cat "$AURA_LAN_FIXTURE_PROVIDER_REPORT")" == filesystem-fallback ]]
kill -0 "$(cat "$AURA_E2E_RUN_DIR/repl.pid")"
# The long-lived service must not inherit the lifecycle descriptor. An
# independent contender can acquire it immediately while that service runs.
(
  exec 8>>"$AURA_E2E_RUN_DIR.lifecycle.lock"
  case "$(uname -s)" in Darwin) /usr/bin/lockf -t 0 8 ;; Linux) flock -n 8 ;; *) exit 1 ;; esac
)
runs="$test_root/.tmp/e2e/run/host-a/artifacts/runs"
manifest="$runs/$AURA_E2E_RUN_TOKEN/.aura-retention.json"
[[ "$(jq -r .state "$manifest")" == active ]]
if bash "$driver" finish success >/dev/null 2>&1; then
  echo 'LAN run finished while tool_repl was active' >&2; exit 1
fi
bash "$driver" stop >/dev/null
if bash "$driver" req '{"method":"ui_state","params":{"instance_id":"absent"}}' 1 >/dev/null 2>&1; then
  echo 'LAN driver accepted a request after shutdown' >&2; exit 1
fi
bash "$driver" finish success >/dev/null
[[ "$(jq -r .outcome "$manifest")" == success ]]
if bash "$driver" start "$test_root/configs/lan.toml" >/dev/null 2>&1; then
  echo 'LAN driver reused a completed run bundle' >&2; exit 1
fi

export AURA_E2E_RUN_TOKEN=lan-retention-test-2
bash "$driver" start "$test_root/configs/lan.toml" >/dev/null
read -r ready <&9
owned_pid=$(cat "$AURA_E2E_RUN_DIR/repl.pid")
cp "$AURA_E2E_RUN_DIR/repl.identity.json" "$test_root/valid-identity"
for field in birth executable run_token; do
  jq --arg field "$field" '.[$field]="mismatched-value"' "$test_root/valid-identity" > "$AURA_E2E_RUN_DIR/repl.identity.json"
  if bash "$driver" stop >/dev/null 2>&1; then
    echo "LAN driver ignored mismatched $field" >&2; exit 1
  fi
  kill -0 "$owned_pid"
done
cp "$test_root/valid-identity" "$AURA_E2E_RUN_DIR/repl.identity.json"
bash "$driver" stop >/dev/null
bash "$driver" finish failed >/dev/null
[[ "$(jq -r .outcome "$runs/$AURA_E2E_RUN_TOKEN/.aura-retention.json")" == failed ]]
# A stored PID can name an unrelated or reused process. Keep every byte of
# run evidence and do not signal it when launch identity does not match.
mkdir -p "$AURA_E2E_RUN_DIR"
printf '%s\n' "$$" > "$AURA_E2E_RUN_DIR/repl.pid"
jq -cn --argjson pid "$$" --arg checkout "$test_root" \
  '{pid:$pid,checkout:$checkout,birth:"stale-birth",executable:"not-this-process",config:"unused"}' > "$AURA_E2E_RUN_DIR/repl.identity.json"
cp "$AURA_E2E_RUN_DIR/repl.identity.json" "$test_root/stale-identity"
if bash "$driver" stop > "$test_root/stop-out" 2> "$test_root/stop-err"; then
  echo 'LAN driver signalled a stale/reused PID' >&2; exit 1
fi
kill -0 "$$"
cmp "$test_root/stale-identity" "$AURA_E2E_RUN_DIR/repl.identity.json"
rm "$AURA_E2E_RUN_DIR/repl.pid" "$AURA_E2E_RUN_DIR/repl.identity.json"

# Deterministic process inspection records reproduce missing entire driver
# state. These fixtures never launch or signal the synthetic PID.
mkdir -p "$test_root/inspect-bin"
real_ps=$(command -v ps)
real_lsof=$(command -v lsof)
export AURA_LAN_FIXTURE_REAL_PS="$real_ps" AURA_LAN_FIXTURE_REAL_LSOF="$real_lsof"
export AURA_LAN_FIXTURE_PROCESS_LIST="$test_root/process-list"
export AURA_LAN_FIXTURE_PROCESS_CWD="$test_root"
cat > "$test_root/inspect-bin/ps" <<'EOF'
#!/usr/bin/env bash
if [[ "$*" == '-axo pid=,comm=' ]]; then cat "$AURA_LAN_FIXTURE_PROCESS_LIST";
else exec "$AURA_LAN_FIXTURE_REAL_PS" "$@"; fi
EOF
cat > "$test_root/inspect-bin/lsof" <<'EOF'
#!/usr/bin/env bash
if [[ "$*" == *' -p 424242 '* ]]; then
  if [[ "$*" == *' -d cwd '* ]]; then printf 'n%s\n' "$AURA_LAN_FIXTURE_PROCESS_CWD";
  else printf 'n%s\n' "${AURA_LAN_FIXTURE_OPEN_PATH:-/unrelated/user-node.sock}"; fi
else exec "$AURA_LAN_FIXTURE_REAL_LSOF" "$@"; fi
EOF
chmod +x "$test_root/inspect-bin/ps" "$test_root/inspect-bin/lsof"
original_path=$PATH
export PATH="$test_root/inspect-bin:$PATH"
printf '424242 /owned/tool_repl\n' > "$AURA_LAN_FIXTURE_PROCESS_LIST"
mv "$AURA_E2E_RUN_DIR" "$test_root/preserved-run"
if bash "$driver" stop > "$test_root/stop-out" 2> "$test_root/stop-err"; then
  echo 'LAN driver claimed stopped with an owned orphan' >&2; exit 1
fi
[[ ! -e "$AURA_E2E_RUN_DIR" ]]
grep -q 'owned LAN harness processes remain' "$test_root/stop-err"
export AURA_E2E_RUN_TOKEN=lan-retention-test-orphan
if bash "$driver" start "$test_root/configs/lan.toml" >/dev/null 2>&1; then
  echo 'LAN driver started over an owned orphan' >&2; exit 1
fi
[[ ! -e "$AURA_E2E_RUN_DIR" ]]
# A user node in the same checkout with no harness-owned open path is not
# owned by the driver and is never signalled.
printf '424242 /owned/aura\n' > "$AURA_LAN_FIXTURE_PROCESS_LIST"
bash "$driver" stop >/dev/null
[[ ! -e "$AURA_E2E_RUN_DIR" ]]
export AURA_LAN_FIXTURE_PROCESS_CWD="$test_root/.tmp/e2e/run/host-a/state/actor"
export AURA_LAN_FIXTURE_OPEN_PATH="$test_root/.tmp/harness/transient/run/actor/command.sock"
if bash "$driver" stop >/dev/null 2>&1; then
  echo 'LAN driver ignored owned native orphan IPC evidence' >&2; exit 1
fi
export AURA_LAN_FIXTURE_PROCESS_CWD="$test_root/sibling-worktree"
export AURA_LAN_FIXTURE_OPEN_PATH="$test_root/sibling-worktree/.tmp/harness/transient/run/actor/command.sock"
bash "$driver" stop >/dev/null
export PATH="$original_path"
mv "$test_root/preserved-run" "$AURA_E2E_RUN_DIR"

# Hold an actual driver inside its owned process inspection, after its lockf
# helper has exited. A competing driver must fail closed for every lifecycle
# command. The fixture inspection child closes FD9 so forced owner death tests
# the driver's inherited open-description custody directly.
mkdir -p "$test_root/lock-bin"
mkfifo "$test_root/lock-ready" "$test_root/lock-release" "$test_root/lock-done"
exec 7<>"$test_root/lock-ready" 6<>"$test_root/lock-release" 5<>"$test_root/lock-done"
cat > "$test_root/lock-bin/ps" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${AURA_LAN_FIXTURE_LOCK_BARRIER:-}" == yes && "$*" == '-axo pid=,comm=' ]]; then
  exec 9>&-
  printf '%s\n' "$$" > "$AURA_LAN_FIXTURE_LOCK_READY"
  read -r release < "$AURA_LAN_FIXTURE_LOCK_RELEASE"
  printf 'done\n' > "$AURA_LAN_FIXTURE_LOCK_DONE"
fi
exec "$AURA_LAN_FIXTURE_REAL_PS" "$@"
EOF
chmod +x "$test_root/lock-bin/ps"
export PATH="$test_root/lock-bin:$original_path"
AURA_LAN_FIXTURE_LOCK_BARRIER=yes \
  AURA_LAN_FIXTURE_LOCK_READY="$test_root/lock-ready" \
  AURA_LAN_FIXTURE_LOCK_RELEASE="$test_root/lock-release" \
  AURA_LAN_FIXTURE_LOCK_DONE="$test_root/lock-done" \
  bash "$driver" stop > "$test_root/lock-owner-out" 2> "$test_root/lock-owner-err" &
lock_owner=$!
read -r inspection_pid <&7
kill -0 "$lock_owner"
for lifecycle in stop 'finish success' "finalize $AURA_E2E_RUN_TOKEN success" "start $test_root/configs/lan.toml"; do
  read -r -a lifecycle_args <<< "$lifecycle"
  if bash "$driver" "${lifecycle_args[@]}" > "$test_root/lock-contender-out" 2>&1; then
    echo "LAN lifecycle command bypassed live kernel owner: $lifecycle" >&2; exit 1
  fi
  grep -q 'another LAN lifecycle owner holds the lock' "$test_root/lock-contender-out"
done
kill -KILL "$lock_owner"
wait "$lock_owner" 2>/dev/null || true
lock_owner=''
# Kernel release permits the next driver despite the persistent lock inode.
bash "$driver" stop >/dev/null
[[ -f "$AURA_E2E_RUN_DIR.lifecycle.lock" ]]
printf 'release\n' >&6
read -r done <&5
[[ "$done" == done ]]
export PATH="$original_path"

# Simulate a replacement during pinned Nix bootstrap. The old outer marker
# was valid, but the driver must validate the expected token after bootstrap
# and leave the replacement process and every launch-identity byte untouched.
export AURA_E2E_RUN_TOKEN=lan-retention-replaced-run
bash "$driver" start "$test_root/configs/lan.toml" >/dev/null
read -r ready <&9
replacement_pid=$(cat "$AURA_E2E_RUN_DIR/repl.pid")
cp "$AURA_E2E_RUN_DIR/repl.identity.json" "$test_root/replacement-identity"
cp "$AURA_E2E_RUN_DIR/retention-run-id" "$test_root/replacement-token"
outer_token=lan-retention-outer-batch
printf '%s\n' "$outer_token" > "$AURA_E2E_RUN_DIR/retention-run-id"
[[ $(cat "$AURA_E2E_RUN_DIR/retention-run-id") == "$outer_token" ]]
cat > "$test_root/replacement-nix" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == develop && "$3" == --command ]]
cp "$AURA_LAN_FIXTURE_REPLACEMENT_TOKEN" "$AURA_E2E_RUN_DIR/retention-run-id"
export IN_NIX_SHELL=fixture
shift 3
exec "$@"
EOF
chmod +x "$test_root/replacement-nix"
if IN_NIX_SHELL= AURA_NIX_BIN="$test_root/replacement-nix" \
  AURA_LAN_FIXTURE_REPLACEMENT_TOKEN="$test_root/replacement-token" \
  bash "$driver" finalize "$outer_token" success > "$test_root/replacement-out" 2>&1; then
  echo 'LAN finalization signalled a replacement run after bootstrap' >&2; exit 1
fi
kill -0 "$replacement_pid"
cmp "$test_root/replacement-identity" "$AURA_E2E_RUN_DIR/repl.identity.json"
[[ $(jq -r .state "$runs/$AURA_E2E_RUN_TOKEN/.aura-retention.json") == active ]]
bash "$driver" finalize "$AURA_E2E_RUN_TOKEN" failed >/dev/null
[[ $(jq -r .outcome "$runs/$AURA_E2E_RUN_TOKEN/.aura-retention.json") == failed ]]

export AURA_E2E_RUN_TOKEN=short
if bash "$driver" start "$test_root/configs/lan.toml" >/dev/null 2>&1; then
  echo 'LAN driver accepted a token too short for the native harness' >&2; exit 1
fi
[[ ! -e "$runs/short" ]]
bash "$repo_root/scripts/harness/lan/test-driver-environment.sh"
bash "$repo_root/scripts/harness/lan/test-fresh.sh"
bash "$repo_root/scripts/harness/lan/test-batch.sh"
echo 'LAN retention lifecycle tests passed'
