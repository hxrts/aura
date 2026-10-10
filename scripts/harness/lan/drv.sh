#!/usr/bin/env bash
# Background tool_repl driver for multi-host harness runs. It runs without a
# window or focus, reading requests from a FIFO. Settings: see env.sh.
# Usage:
#   drv.sh start <config>        start tool_repl in the background
#   drv.sh req '<json-no-id>' [timeout-s]
#                                send a request, print the matching response line
#   drv.sh stop                  shut down the REPL and its children
#   drv.sh finish success|failed record outcome after evidence capture
#   drv.sh finalize <token> success|failed
#                                stop and finish only the exact owned batch
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=env.sh
. "$here/env.sh"
cd "$AURA_E2E_ROOT"
# Raw SSH commands must use the same pinned dependencies as local LAN runs.
# Bootstrap before inspecting identities or mutating any retained run state.
if [[ -z "${IN_NIX_SHELL:-}" ]]; then
  nix_bin="${AURA_NIX_BIN:-/nix/var/nix/profiles/default/bin/nix}"
  exec "$nix_bin" develop "$AURA_E2E_ROOT" --command bash "$here/drv.sh" "$@"
fi
command -v jq >/dev/null 2>&1 || {
  echo 'LAN driver requires jq in the pinned Nix environment; no state changed' >&2
  exit 127
}
case "${1:-}" in
  start|stop|finish|finalize)
    command -v flock >/dev/null 2>&1 || {
      echo 'LAN driver requires portable flock in the pinned Nix environment; no state changed' >&2
      exit 127
    }
    ;;
esac
D=$AURA_E2E_RUN_DIR
FIFO=$D/repl.in OUT=$D/repl.out PIDF=$D/repl.pid SEQ=$D/repl.seq
IDENTITY=$D/repl.identity.json
physical_repo="$(pwd -P)"
RETENTION_ROOT_FILE=$D/retention-root
RETENTION_RUN_FILE=$D/retention-run-id
RETENTION_TOOL=$AURA_E2E_ROOT/scripts/dev/retain-e2e-runs.sh

# Keep one persistent lock inode outside the replaceable run directory. Kernel
# custody releases dead holders; a leftover file does not constitute a lock.
# FD9 is closed on the background service so it cannot retain this short owner.
case "${1:-}" in
  start|stop|finish|finalize)
    mkdir -p "$(dirname "$D")"
    lifecycle_lock="$D.lifecycle.lock"
    [[ ! -L "$lifecycle_lock" ]] || { echo 'LAN lifecycle lock is a symlink; refusing mutation' >&2; exit 1; }
    exec 9>>"$lifecycle_lock"
    flock -n 9 || { echo 'another LAN lifecycle owner holds the lock; refusing mutation' >&2; exit 1; }
    ;;
esac

require_run_token() {
  local expected=$1
  [[ "$expected" =~ ^[a-z0-9][a-z0-9-]*$ && ${#expected} -ge 16 &&
     -f "$RETENTION_RUN_FILE" && ! -L "$RETENTION_RUN_FILE" &&
     "$(cat "$RETENTION_RUN_FILE")" == "$expected" ]] || {
    echo 'batch run identity is absent or different; retaining state without signalling' >&2; return 1;
  }
}

# PID presence is insufficient authority to signal a process: the PID may
# have been reused. Keep the launch birth and exact command/cwd evidence.
process_birth() { LC_ALL=C ps -p "$1" -o lstart= 2>/dev/null | sed 's/^[[:space:]]*//;s/[[:space:]]*$//'; }
process_cwd() { lsof -a -p "$1" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p' | head -n 1; }
verify_repl_identity() {
  local pid=$1 birth expected_birth expected_exe expected_config cwd args run_token
  [[ "$pid" =~ ^[1-9][0-9]*$ && -f "$IDENTITY" ]] || return 1
  run_token=$(cat "$RETENTION_RUN_FILE") || return 1
  jq -e --argjson pid "$pid" --arg root "$physical_repo" \
    --arg token "$run_token" \
    '.pid==$pid and .checkout==$root and .run_token==$token' "$IDENTITY" >/dev/null || return 1
  birth=$(process_birth "$pid") || return 1
  expected_birth=$(jq -er '.birth' "$IDENTITY") || return 1
  [[ -n "$birth" && "$birth" == "$expected_birth" ]] || return 1
  cwd=$(process_cwd "$pid") || return 1
  [[ "$cwd" == "$physical_repo" ]] || return 1
  expected_exe=$(jq -er '.executable' "$IDENTITY") || return 1
  expected_config=$(jq -er '.config' "$IDENTITY") || return 1
  args=$(ps -p "$pid" -o args= 2>/dev/null) || return 1
  # Bash scripts are permitted for isolated fixture/explicit operator binaries.
  # Both shapes require the exact launched binary and config, never a basename.
  [[ "$args" == "$expected_exe --config $expected_config "* || \
     "$args" == *"bash $expected_exe --config $expected_config "* ]]
}

stop_owned_run() {
  local expected=${1:-} repl_pid
  [[ -z "$expected" ]] || require_run_token "$expected"
  if [[ -f "$PIDF" ]]; then
    repl_pid=$(cat "$PIDF")
    verify_repl_identity "$repl_pid" || {
      echo 'LAN PID ownership is missing/stale/mismatched; retaining state without signalling' >&2; return 1;
    }
    # TERM enters the REPL's shared stop_all owner. Revalidate exact batch and
    # PID birth/cwd/executable together immediately before signalling.
    [[ -z "$expected" ]] || require_run_token "$expected"
    verify_repl_identity "$repl_pid" || return 1
    kill -TERM "$repl_pid"
    for _ in {1..20}; do
      kill -0 "$repl_pid" 2>/dev/null || break
      sleep 0.1
    done
    if kill -0 "$repl_pid" 2>/dev/null; then
      echo "LAN REPL $repl_pid did not stop; retaining active run state" >&2
      return 1
    fi
  elif [[ -f "$IDENTITY" ]]; then
    echo 'LAN launch identity exists without PID state; retaining evidence for inspection' >&2; return 1
  fi
  require_no_owned_harness
  [[ -z "$expected" ]] || require_run_token "$expected"
  rm -f "$PIDF" "$IDENTITY"
  echo stopped
}

finish_owned_run() {
  local outcome=$1 expected=${2:-} run_token runs_root
  [[ "$outcome" == success || "$outcome" == failed ]] || { echo 'finish outcome must be success or failed' >&2; return 2; }
  require_no_owned_harness
  [[ ! -f "$PIDF" && ! -f "$IDENTITY" ]] || { echo 'stop the LAN driver before recording an outcome' >&2; return 1; }
  [[ -f "$RETENTION_ROOT_FILE" && -f "$RETENTION_RUN_FILE" ]] || {
    echo 'this LAN run has no retention metadata; inspect it manually' >&2; return 1;
  }
  run_token=$(cat "$RETENTION_RUN_FILE")
  runs_root=$(cat "$RETENTION_ROOT_FILE")
  [[ -z "$expected" ]] || require_run_token "$expected"
  bash "$RETENTION_TOOL" --root "$runs_root" finish "$run_token" "$outcome"
}
owned_harness_processes() {
  local pid executable cwd open_path name process_list
  command -v ps >/dev/null && command -v lsof >/dev/null || {
    echo 'cannot inspect owned LAN processes; refusing lifecycle operation' >&2; return 1;
  }
  process_list=$(ps -axo pid=,comm=) || { echo 'cannot inspect LAN process list' >&2; return 1; }
  while read -r pid executable; do
    [[ "$pid" =~ ^[1-9][0-9]*$ ]] || continue
    name=${executable##*/}
    case "$name" in tool_repl|aura-harness|aura|"${AURA_E2E_TOOL_REPL##*/}") ;; *) continue ;; esac
    cwd=$(process_cwd "$pid" || true)
    case "$name" in
      tool_repl|aura-harness|"${AURA_E2E_TOOL_REPL##*/}")
        [[ -n "$cwd" ]] || { echo "cannot inspect harness candidate $pid" >&2; return 1; }
        [[ "$cwd" == "$physical_repo" ]] && echo "$pid"
        ;;
      aura)
        while IFS= read -r open_path; do
          case "$open_path" in
            "n$physical_repo/.tmp/harness/transient/"*|"n$D/"*) echo "$pid"; break ;;
          esac
        done < <(lsof -a -p "$pid" -Fn 2>/dev/null || true)
        ;;
    esac
  done <<< "$process_list"
  return 0
}
require_no_owned_harness() {
  local active
  # Read-only inspection does not own lifecycle mutation. Avoid leaving a
  # descriptor owner in its command-substitution shell if this driver dies.
  active=$(exec 9>&-; owned_harness_processes) || return 1
  [[ -z "$active" ]] || {
    echo "owned LAN harness processes remain ($active); retaining state, inspect their exact ownership before recovery" >&2; return 1;
  }
}

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
  # Never overwrite FIFO/evidence under an active or unidentified old owner.
  if [[ -f "$PIDF" || -f "$IDENTITY" ]]; then
    echo 'existing LAN driver identity; stop/inspect it before starting a new run' >&2; exit 1;
  fi
  require_no_owned_harness
  mkdir -p "$D"
  # Configs name this host's LAN address as __HOST_ADDR__; render a copy.
  if grep -q __HOST_ADDR__ "$2"; then
    [ -n "$AURA_E2E_HOST_ADDR" ] || { echo "set AURA_E2E_HOST_ADDR to this host's LAN address" >&2; exit 2; }
    rendered="$D/$(basename "$2")"
    sed "s/__HOST_ADDR__/$AURA_E2E_HOST_ADDR/g" "$2" > "$rendered"
    set -- "$1" "$rendered"
  fi
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
  export AURA_HARNESS_WEB_PREBUILT_ONLY=1
  # Explicit provider configuration avoids OS credential prompts. The runtime
  # still requires its existing harness admission for filesystem fallback.
  export AURA_SECURE_STORAGE_BACKEND=filesystem-fallback
  # Expose the browser transport relay on this host's LAN address (taken from the config's
  # non-loopback bind_address) so peers on the other host reach its browsers.
  lan_host=$(grep -o 'bind_address = "[^"]*"' "$2" | cut -d'"' -f2 | cut -d: -f1 | grep -v "^127\." | head -1 || true)
  [ -n "$lan_host" ] && export AURA_HARNESS_WEB_RELAY_HOST="$lan_host"
  # Hold the FIFO open so the REPL never sees EOF.
  nohup bash -c 'exec 3<>"$1"; exec nice -n 5 "$2" --config "$3" --idle-timeout-ms 0 <&3' \
    lan-tool-repl "$FIFO" "$AURA_E2E_TOOL_REPL" "$2" \
    >"$OUT" 2>"$D/repl.err" </dev/null 9>&- &
  repl_pid=$!
  birth=$(process_birth "$repl_pid")
  [[ -n "$birth" ]] || { echo 'LAN process exited before launch identity was captured' >&2; exit 1; }
  jq -cn --argjson pid "$repl_pid" --arg birth "$birth" --arg checkout "$physical_repo" \
    --arg executable "$AURA_E2E_TOOL_REPL" --arg config "$2" --arg token "$AURA_E2E_RUN_TOKEN" \
    '{pid:$pid,birth:$birth,checkout:$checkout,executable:$executable,config:$config,run_token:$token}' > "$IDENTITY"
  printf '%s\n' "$repl_pid" > "$PIDF"
  echo "started pid $repl_pid"
  ;;
req)
  [[ -f "$PIDF" ]] && kill -0 "$(cat "$PIDF")" 2>/dev/null || {
    echo 'LAN REPL is not running; request rejected' >&2; exit 1;
  }
  verify_repl_identity "$(cat "$PIDF")" || {
    echo 'LAN REPL ownership does not match launch identity; request rejected' >&2; exit 1;
  }
  n=$(( $(cat "$SEQ") + 1 )); echo $n > "$SEQ"
  body="${2#\{}"
  printf '{"id":%s,%s\n' "$n" "$body" > "$FIFO" &
  request_writer=$!
  ingress_deadline=$((SECONDS + 5))
  while kill -0 "$request_writer" 2>/dev/null; do
    if ! kill -0 "$(cat "$PIDF")" 2>/dev/null || (( SECONDS >= ingress_deadline )); then
      kill "$request_writer" 2>/dev/null || true
      wait "$request_writer" 2>/dev/null || true
      echo 'LAN request ingress failed: REPL exited or FIFO admission timed out' >&2
      exit 1
    fi
    sleep 0.05
  done
  wait "$request_writer"
  to=${3:-120}
  for _ in $(seq 1 $((to*5))); do
    line=$(grep -m1 "^{\"id\":$n[,}]" "$OUT" 2>/dev/null || true)
    [ -n "$line" ] && { echo "$line"; exit 0; }
    kill -0 "$(cat "$PIDF")" 2>/dev/null || {
      echo "LAN REPL exited before response id=$n" >&2; exit 1;
    }
    sleep 0.2
  done
  echo "TIMEOUT id=$n" >&2; exit 1
  ;;
stop)
  stop_owned_run
  ;;
finish)
  [[ "${2:-}" == success || "${2:-}" == failed ]] || { echo 'usage: drv.sh finish success|failed' >&2; exit 2; }
  finish_owned_run "$2"
  ;;
finalize)
  [[ $# == 3 && ( "$3" == success || "$3" == failed ) ]] || { echo 'usage: drv.sh finalize <expected-token> success|failed' >&2; exit 2; }
  require_run_token "$2"
  stop_owned_run "$2"
  finish_owned_run "$3" "$2"
  ;;
*) echo "usage: $0 start <config>|req <json> [timeout]|stop|finish success|failed|finalize <token> success|failed" >&2; exit 2;;
esac
