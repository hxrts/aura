#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

require_command() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "[demo-smoke] required command not found: $cmd" >&2
    exit 1
  fi
}

require_command curl
require_command lsof

pick_port() {
  local port=""
  for port in $(seq 4279 4299); do
    if ! lsof -PiTCP:"$port" -sTCP:LISTEN -t >/dev/null 2>&1; then
      echo "$port"
      return 0
    fi
  done
  echo "[demo-smoke] failed to find a free demo web port in 4279-4299" >&2
  exit 1
}

port="$(pick_port)"
log_file="$(mktemp -t aura-demo-smoke.XXXXXX.log)"
launcher_pid=""

cleanup() {
  if [[ -n "$launcher_pid" ]] && kill -0 "$launcher_pid" 2>/dev/null; then
    kill -INT "$launcher_pid" 2>/dev/null || true
    wait "$launcher_pid" 2>/dev/null || true
  fi
  rm -f "$log_file"
}

trap cleanup EXIT INT TERM

wait_for_ready() {
  local ready="0"
  for _ in $(seq 1 180); do
    if ! kill -0 "$launcher_pid" 2>/dev/null; then
      echo "[demo-smoke] launcher exited before becoming ready" >&2
      cat "$log_file" >&2 || true
      exit 1
    fi
    if curl -fsS "http://127.0.0.1:${port}/" >/dev/null 2>&1 \
      && grep -F "[demo] web-only mode is active; press Ctrl+C to stop" "$log_file" >/dev/null 2>&1; then
      ready="1"
      break
    fi
    sleep 1
  done

  if [[ "$ready" != "1" ]]; then
    echo "[demo-smoke] timed out waiting for launcher readiness" >&2
    cat "$log_file" >&2 || true
    exit 1
  fi
}

assert_profile_isolation() {
  local profile_dir="$repo_root/.tmp/demo/web-profile"
  local pid_file="$repo_root/.tmp/demo/pids/web-static.pid"

  [[ -d "$profile_dir" ]] || {
    echo "[demo-smoke] expected browser profile dir at $profile_dir" >&2
    exit 1
  }

  [[ -f "$pid_file" ]] || {
    echo "[demo-smoke] expected owned web pid file at $pid_file" >&2
    exit 1
  }

  grep -F "[demo] browser profile: $profile_dir" "$log_file" >/dev/null 2>&1 || {
    echo "[demo-smoke] launcher did not print the dedicated browser profile path" >&2
    cat "$log_file" >&2 || true
    exit 1
  }

  grep -F -- "--user-data-dir=\"$profile_dir\"" "$log_file" >/dev/null 2>&1 || {
    echo "[demo-smoke] manual browser command did not carry the dedicated profile path" >&2
    cat "$log_file" >&2 || true
    exit 1
  }
}

stop_and_assert_cleanup() {
  local pid_file="$repo_root/.tmp/demo/pids/web-static.pid"

  kill -INT "$launcher_pid" 2>/dev/null || true
  wait "$launcher_pid" || true
  launcher_pid=""

  if lsof -PiTCP:"$port" -sTCP:LISTEN -t >/dev/null 2>&1; then
    echo "[demo-smoke] demo web port $port is still listening after cleanup" >&2
    lsof -nP -iTCP:"$port" -sTCP:LISTEN >&2 || true
    exit 1
  fi

  if [[ -f "$pid_file" ]]; then
    echo "[demo-smoke] expected owned pid file to be removed after cleanup" >&2
    exit 1
  fi
}

run_cycle() {
  local cycle="$1"
  : >"$log_file"
  echo "[demo-smoke] starting cycle $cycle on port $port"
  ./scripts/dev/demo-dual.sh --mode web --browser none --web-port "$port" --reset 1 >"$log_file" 2>&1 &
  launcher_pid=$!
  wait_for_ready
  assert_profile_isolation
  stop_and_assert_cleanup
}

run_cycle 1
run_cycle 2

echo "[demo-smoke] launcher startup, profile isolation, and rerun cleanup checks passed"
