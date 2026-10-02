#!/usr/bin/env bash
# Background tool_repl driver for multi-host harness runs. It runs without a
# window or focus, reading requests from a FIFO. Settings: see env.sh.
# Usage:
#   drv.sh start <config>        start tool_repl in the background
#   drv.sh req '<json-no-id>' [timeout-s]
#                                send a request, print the matching response line
#   drv.sh stop                  shut down the REPL and its children
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=env.sh
. "$here/env.sh"
cd "$AURA_E2E_ROOT"
D=$AURA_E2E_RUN_DIR
mkdir -p "$D"
FIFO=$D/repl.in OUT=$D/repl.out PIDF=$D/repl.pid SEQ=$D/repl.seq

case "${1:-}" in
start)
  [ -n "${2:-}" ] || { echo "usage: $0 start <config>" >&2; exit 2; }
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
    pkill -TERM -P "$(cat "$PIDF")" 2>/dev/null || true
    kill "$(cat "$PIDF")" 2>/dev/null || true
  fi
  rm -f "$PIDF"; echo stopped
  ;;
*) echo "usage: $0 start <config>|req <json> [timeout]|stop" >&2; exit 2;;
esac
