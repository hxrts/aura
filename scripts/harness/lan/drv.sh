#!/usr/bin/env bash
# Background tool_repl driver for multi-host harness runs. It runs without a
# window or focus, reading requests from a FIFO. Settings: see env.sh.
# Usage:
#   drv.sh start <config>        start tool_repl in the background
#   drv.sh req '<json-no-id>' [timeout-s]
#                                send a request, print the matching response line
#   drv.sh stop                  shut down the REPL and its children
#   drv.sh finish success|failed record outcome after evidence capture
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=env.sh
. "$here/env.sh"
cd "$AURA_E2E_ROOT"
D=$AURA_E2E_RUN_DIR
mkdir -p "$D"
FIFO=$D/repl.in OUT=$D/repl.out PIDF=$D/repl.pid SEQ=$D/repl.seq
RETENTION_ROOT_FILE=$D/retention-root
RETENTION_RUN_FILE=$D/retention-run-id
RETENTION_TOOL=$AURA_E2E_ROOT/scripts/dev/retain-e2e-runs.sh

retention_root_for_config() {
  local artifact_dir runs_root
  artifact_dir="$(awk -F '"' '/^[[:space:]]*artifact_dir[[:space:]]*=/ {print $2; exit}' "$1")"
  [[ -n "$artifact_dir" && "$artifact_dir" != *..* ]] || {
    echo 'LAN config needs a safe [run].artifact_dir' >&2; return 1;
  }
  if [[ "$artifact_dir" == /* ]]; then runs_root="$artifact_dir/runs";
  else runs_root="$AURA_E2E_ROOT/$artifact_dir/runs"; fi
  mkdir -p "$runs_root"
  runs_root="$(cd "$runs_root" && pwd -P)"
  local physical_repo
  physical_repo="$(cd "$AURA_E2E_ROOT" && pwd -P)"
  case "$runs_root" in
    "$physical_repo"/.tmp/e2e/run/*/artifacts/runs) printf '%s\n' "$runs_root" ;;
    *) echo "LAN artifact root is outside the owned run tree: $runs_root" >&2; return 1 ;;
  esac
}

case "${1:-}" in
start)
  [ -n "${2:-}" ] || { echo "usage: $0 start <config>" >&2; exit 2; }
  [[ "$AURA_E2E_RUN_TOKEN" =~ ^[a-z0-9][a-z0-9-]*$ ]] || {
    echo 'LAN run token must be lowercase letters, digits and hyphens' >&2; exit 2;
  }
  (( ${#AURA_E2E_RUN_TOKEN} >= 16 )) || {
    echo 'LAN run token must be at least 16 bytes for the native harness' >&2; exit 2;
  }
  runs_root="$(retention_root_for_config "$2")"
  run_bundle="$runs_root/$AURA_E2E_RUN_TOKEN"
  [[ ! -e "$run_bundle" && ! -L "$run_bundle" ]] || {
    echo "LAN run bundle already exists; use a new token: $run_bundle" >&2; exit 1;
  }
  mkdir "$run_bundle"
  bash "$RETENTION_TOOL" --root "$runs_root" begin "$AURA_E2E_RUN_TOKEN"
  printf '%s\n' "$runs_root" > "$RETENTION_ROOT_FILE"
  printf '%s\n' "$AURA_E2E_RUN_TOKEN" > "$RETENTION_RUN_FILE"
  rm -f "$FIFO" "$OUT"; mkfifo "$FIFO"; echo 0 > "$SEQ"
  export AURA_HARNESS_AURA_BIN="${AURA_HARNESS_AURA_BIN:-$here/aura-wrap.sh}"
  export AURA_HARNESS_RUN_TOKEN="$AURA_E2E_RUN_TOKEN"
  # Expose the browser transport relay on this host's LAN address (taken from the config's
  # non-loopback bind_address) so peers on the other host reach its browsers.
  lan_host=$(grep -o 'bind_address = "[^"]*"' "$2" | cut -d'"' -f2 | cut -d: -f1 | grep -v "^127\." | head -1 || true)
  [ -n "$lan_host" ] && export AURA_HARNESS_WEB_RELAY_HOST="$lan_host"
  # Hold the FIFO open so the REPL never sees EOF.
  nohup bash -c "exec 3<>'$FIFO'; exec nice -n 5 '$AURA_E2E_TOOL_REPL' --config '$2' --idle-timeout-ms 0 <&3" \
    >"$OUT" 2>"$D/repl.err" </dev/null &
  echo $! > "$PIDF"; echo "started pid $(cat "$PIDF")"
  ;;
req)
  n=$(( $(cat "$SEQ") + 1 )); echo $n > "$SEQ"
  body="${2#\{}"
  printf '{"id":%s,%s\n' "$n" "$body" > "$FIFO"
  to=${3:-120}
  for _ in $(seq 1 $((to*5))); do
    line=$(grep -m1 "^{\"id\":$n[,}]" "$OUT" 2>/dev/null || true)
    [ -n "$line" ] && { echo "$line"; exit 0; }
    sleep 0.2
  done
  echo "TIMEOUT id=$n" >&2; exit 1
  ;;
stop)
  # Only stop the REPL this driver started (and its children).
  if [ -f "$PIDF" ]; then
    repl_pid="$(cat "$PIDF")"
    pkill -TERM -P "$repl_pid" 2>/dev/null || true
    kill "$repl_pid" 2>/dev/null || true
    for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
      kill -0 "$repl_pid" 2>/dev/null || break
      sleep 0.1
    done
    if kill -0 "$repl_pid" 2>/dev/null; then
      echo "LAN REPL $repl_pid did not stop; retaining active run state" >&2
      exit 1
    fi
  fi
  rm -f "$PIDF"; echo stopped
  ;;
finish)
  [[ "${2:-}" == success || "${2:-}" == failed ]] || { echo 'usage: drv.sh finish success|failed' >&2; exit 2; }
  [[ ! -f "$PIDF" ]] || { echo 'stop the LAN driver before recording an outcome' >&2; exit 1; }
  [[ -f "$RETENTION_ROOT_FILE" && -f "$RETENTION_RUN_FILE" ]] || {
    echo 'this LAN run has no retention metadata; inspect it manually' >&2; exit 1;
  }
  bash "$RETENTION_TOOL" --root "$(cat "$RETENTION_ROOT_FILE")" \
    finish "$(cat "$RETENTION_RUN_FILE")" "$2"
  ;;
*) echo "usage: $0 start <config>|req <json> [timeout]|stop|finish success|failed" >&2; exit 2;;
esac
