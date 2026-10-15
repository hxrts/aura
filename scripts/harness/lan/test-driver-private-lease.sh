#!/usr/bin/env bash
# Exact local lifecycle fixture: private helpers must release before finalization.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
driver=${AURA_E2E_DRIVER_FOR_TEST:-$here/drv.sh}
fixture=$(mktemp -d /private/tmp/aura-driver-lease.XXXXXX)
cleanup() {
  for pid in ${helper_pid:-} ${finalizer_pid:-} ${repl_pid:-}; do
    [[ -z "$pid" ]] || kill "$pid" 2>/dev/null || true
  done
  [[ "${KEEP_FIXTURE:-0}" == 1 ]] || rm -rf "$fixture"
}
trap cleanup EXIT
root="$fixture/checkout"
mkdir -p "$root/bin" "$root/scripts/dev" "$root/.tmp/e2e/run/host-a/artifacts/runs"
cp "$here/../../dev/retain-e2e-runs.sh" "$root/scripts/dev/retain-e2e-runs.sh"
cat > "$fixture/tool_repl.c" <<'C'
#include <signal.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

static volatile sig_atomic_t stopped;
static void stop(int signal_number) { (void)signal_number; stopped = 1; }
int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--private-request-socket") == 0) {
        const char *ready = getenv("FIXTURE_READY_FIFO");
        const char *release = getenv("FIXTURE_RELEASE_FIFO");
        FILE *out = fopen(ready, "w");
        if (!out) return 2;
        fputs("ready\n", out);
        fclose(out);
        FILE *in = fopen(release, "r");
        if (!in) return 3;
        (void)fgetc(in);
        fclose(in);
        return 0;
    }
    if (argc < 5 || strcmp(argv[1], "--config") != 0 ||
        strcmp(argv[3], "--private-socket") != 0) return 4;
    int socket_fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (socket_fd < 0) return 5;
    struct sockaddr_un address = {0};
    address.sun_family = AF_UNIX;
    if (strlen(argv[4]) >= sizeof(address.sun_path)) return 6;
    strncpy(address.sun_path, argv[4], sizeof(address.sun_path) - 1);
    if (bind(socket_fd, (struct sockaddr *)&address, sizeof(address)) != 0) return 7;
    if (listen(socket_fd, 1) != 0) return 8;
    FILE *ready = fopen(getenv("FIXTURE_START_READY_FIFO"), "w");
    if (!ready) return 9;
    fputs("started\n", ready);
    fclose(ready);
    signal(SIGTERM, stop);
    while (!stopped) pause();
    close(socket_fd);
    unlink(argv[4]);
    return 0;
}
C
cc -std=c11 -Wall -Wextra -Werror "$fixture/tool_repl.c" -o "$root/bin/tool_repl"
cat > "$fixture/config.toml" <<EOF
[run]
artifact_dir = "$root/.tmp/e2e/run/host-a/artifacts"
EOF
export IN_NIX_SHELL=fixture AURA_E2E_ROOT="$root"
export AURA_E2E_RUN_DIR="$root/.tmp/e2e/run"
export AURA_E2E_TOOL_REPL="$root/bin/tool_repl"
export AURA_E2E_HOST_ADDR=127.0.0.1
export FIXTURE_READY_FIFO="$fixture/ready" FIXTURE_RELEASE_FIFO="$fixture/release"
export FIXTURE_START_READY_FIFO="$fixture/start-ready"
mkfifo "$FIXTURE_READY_FIFO" "$FIXTURE_RELEASE_FIFO" "$FIXTURE_START_READY_FIFO"
exec 4<>"$FIXTURE_READY_FIFO" 5<>"$FIXTURE_START_READY_FIFO"

start_run() {
  export AURA_E2E_RUN_TOKEN=$1
  bash "$driver" start "$fixture/config.toml" > "$fixture/start.out"
  repl_pid=$(cat "$AURA_E2E_RUN_DIR/repl.pid")
  [[ $(jq -r '.run_token' "$AURA_E2E_RUN_DIR/repl.identity.json") == "$AURA_E2E_RUN_TOKEN" ]]
  IFS= read -r -t 5 started <&5
  [[ "$started" == started ]]
  [[ -S "$AURA_E2E_RUN_DIR/private-$AURA_E2E_RUN_TOKEN/owner.sock" ]]
}
hold_helper() {
  bash "$driver" req-private > "$fixture/helper.out" 2> "$fixture/helper.err" &
  helper_pid=$!
  IFS= read -r -t 20 ready <&4
  [[ "$ready" == ready ]]
  kill -0 "$helper_pid"
}
release_helper() {
  printf 'release\n' > "$FIXTURE_RELEASE_FIFO"
  wait "$helper_pid"
  helper_pid=
}

# An expired bounded lease leaves the original owner and retention state intact.
if [[ "${SKIP_TIMEOUT_CASE:-0}" != 1 ]]; then
start_run driver-private-timeout
if [[ "${SKIP_LEASE_NEGATIVES:-0}" != 1 ]]; then
lock="$AURA_E2E_RUN_DIR/private-$AURA_E2E_RUN_TOKEN/request-lifetime.lock"
printf 'untouched\n' > "$fixture/unrelated"
ln -s "$fixture/unrelated" "$lock"
if bash "$driver" req-private > "$fixture/linked.out" 2> "$fixture/linked.err"; then
  echo 'private helper accepted a linked lease' >&2; exit 1
fi
[[ $(cat "$fixture/unrelated") == untouched ]]
unlink "$lock"
printf 'private\n' > "$lock"
chmod 644 "$lock"
if bash "$driver" req-private > "$fixture/mode.out" 2> "$fixture/mode.err"; then
  echo 'private helper accepted a readable lease' >&2; exit 1
fi
unlink "$lock"
fi
hold_helper
if AURA_E2E_PRIVATE_LEASE_WAIT_SECONDS=1 bash "$driver" finalize \
    "$AURA_E2E_RUN_TOKEN" success > "$fixture/timeout.out" 2> "$fixture/timeout.err"; then
  echo 'finalizer crossed a live private request lease' >&2; exit 1
fi
rg -q 'private LAN request lease did not quiesce' "$fixture/timeout.err"
kill -0 "$repl_pid"
[[ -f "$AURA_E2E_RUN_DIR/repl.identity.json" ]]
release_helper
bash "$driver" finalize "$AURA_E2E_RUN_TOKEN" success > "$fixture/finalize.out"
! kill -0 "$repl_pid" 2>/dev/null
[[ ! -f "$AURA_E2E_RUN_DIR/repl.identity.json" ]]
bash "$driver" finalize "$AURA_E2E_RUN_TOKEN" success > "$fixture/finalize-again.out"
jq -e '.state=="completed" and .outcome=="success"' \
  "$root/.tmp/e2e/run/host-a/artifacts/runs/$AURA_E2E_RUN_TOKEN/.aura-retention.json" >/dev/null
fi

# A held helper releases; the finalizer then observes real process shutdown.
start_run driver-private-wait-pass
hold_helper
mkfifo "$fixture/done"
exec 6<>"$fixture/done"
(bash "$driver" finalize "$AURA_E2E_RUN_TOKEN" success > "$fixture/wait.out" 2> "$fixture/wait.err";
  printf 'finished\n' > "$fixture/done") &
finalizer_pid=$!
if IFS= read -r -t 1 _ <&6; then
  echo 'finalizer finished before the private helper released' >&2; exit 1
fi
kill -0 "$repl_pid"
release_helper
IFS= read -r -t 10 completion <&6
[[ "$completion" == finished ]]
wait "$finalizer_pid"
finalizer_pid=
! kill -0 "$repl_pid" 2>/dev/null
[[ ! -f "$AURA_E2E_RUN_DIR/repl.identity.json" ]]
bash "$driver" finalize "$AURA_E2E_RUN_TOKEN" success > "$fixture/wait-again.out"
echo 'LAN private request lifecycle fixture passed'
