#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/dev/demo-dual.sh [options]

Options:
  --mode MODE                dual (default), tui, or web
  --web-port PORT            Static web server port (default: 4173)
  --tui-bind-address ADDR    TUI bind address (default: 127.0.0.1:43101)
  --reset 0|1                Recreate demo state before startup (default: 1)
  --browser BROWSER          auto (default), chrome, chromium, or none
  --help                     Show this help text

Notes:
  - The launcher owns the web server port and the TUI bind address.
  - Existing listeners on those configured ports are stopped before startup.
  - Browser-direct WebSocket transport is still runtime-assigned and flows
    through exported invitation or enrollment codes.
  - Use --browser none to print manual launch instructions without opening a browser.
EOF
}

mode="dual"
web_port="4173"
tui_bind_address="127.0.0.1:43101"
reset="1"
browser="auto"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      mode="${2:-}"
      shift 2
      ;;
    --web-port)
      web_port="${2:-}"
      shift 2
      ;;
    --tui-bind-address)
      tui_bind_address="${2:-}"
      shift 2
      ;;
    --reset)
      reset="${2:-}"
      shift 2
      ;;
    --browser)
      browser="${2:-}"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "[demo] unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$mode" in
  dual|tui|web) ;;
  *)
    echo "[demo] unsupported mode: $mode" >&2
    exit 2
    ;;
esac

case "$reset" in
  0|1) ;;
  *)
    echo "[demo] --reset must be 0 or 1" >&2
    exit 2
    ;;
esac

case "$browser" in
  auto|chrome|chromium|none) ;;
  *)
    echo "[demo] unsupported browser: $browser" >&2
    exit 2
    ;;
esac

if [[ ! "$web_port" =~ ^[0-9]+$ ]]; then
  echo "[demo] --web-port must be numeric" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

demo_root="$repo_root/.tmp/demo"
tui_data_dir="$demo_root/tui-data"
web_profile_dir="$demo_root/web-profile"
logs_dir="$demo_root/logs"
pids_dir="$demo_root/pids"
web_log="$logs_dir/web-static.log"
browser_log="$logs_dir/browser-launch.log"
web_pid_file="$pids_dir/web-static.pid"
meta_file="$pids_dir/demo-meta.env"
web_url="http://127.0.0.1:${web_port}/"
tui_device_id="demo:tui"
manual_browser_cmd=""
browser_display_name=""
web_server_pid=""

require_command() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "[demo] required command not found: $cmd" >&2
    exit 1
  fi
}

port_in_use() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    lsof -PiTCP:"$port" -sTCP:LISTEN -t >/dev/null 2>&1
  else
    return 1
  fi
}

show_port_owner() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1; then
    lsof -nP -iTCP:"$port" -sTCP:LISTEN >&2 || true
  fi
}

stop_port_listener() {
  local port="$1"
  local label="$2"

  if ! command -v lsof >/dev/null 2>&1; then
    if port_in_use "$port"; then
      echo "[demo] $label port $port is already in use and lsof is unavailable to stop it" >&2
      exit 1
    fi
    return 0
  fi

  mapfile -t existing_pids < <(lsof -PiTCP:"$port" -sTCP:LISTEN -t 2>/dev/null || true)
  if [[ "${#existing_pids[@]}" -eq 0 ]]; then
    return 0
  fi

  echo "[demo] stopping existing $label listener(s) on port $port" >&2
  show_port_owner "$port"
  kill "${existing_pids[@]}" 2>/dev/null || true
  for _ in $(seq 1 30); do
    if ! lsof -PiTCP:"$port" -sTCP:LISTEN -t >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.1
  done

  echo "[demo] $label port $port is still busy after SIGTERM; force stopping listener(s)" >&2
  kill -9 "${existing_pids[@]}" 2>/dev/null || true
  sleep 0.1
  if lsof -PiTCP:"$port" -sTCP:LISTEN -t >/dev/null 2>&1; then
    echo "[demo] failed to clear $label port $port" >&2
    show_port_owner "$port"
    exit 1
  fi
}

cleanup_web_server() {
  if [[ -f "$web_pid_file" ]]; then
    local pid
    pid="$(cat "$web_pid_file" 2>/dev/null || true)"
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      for _ in $(seq 1 30); do
        if ! kill -0 "$pid" 2>/dev/null; then
          break
        fi
        sleep 0.1
      done
      if kill -0 "$pid" 2>/dev/null; then
        kill -9 "$pid" 2>/dev/null || true
      fi
    fi
    rm -f "$web_pid_file"
  fi
}

cleanup() {
  cleanup_web_server
}

trap cleanup EXIT INT TERM

prepare_dirs() {
  if [[ "$reset" == "1" ]]; then
    rm -rf "$tui_data_dir" "$web_profile_dir" "$logs_dir" "$pids_dir"
  fi
  mkdir -p "$tui_data_dir" "$web_profile_dir" "$logs_dir" "$pids_dir"
}

write_metadata() {
  cat >"$meta_file" <<EOF
MODE=$mode
WEB_PORT=$web_port
WEB_URL=$web_url
TUI_BIND_ADDRESS=$tui_bind_address
TUI_DATA_DIR=$tui_data_dir
TUI_DEVICE_ID=$tui_device_id
WEB_PROFILE_DIR=$web_profile_dir
WEB_LOG=$web_log
BROWSER_LOG=$browser_log
RESET=$reset
BROWSER=$browser
EOF
}

check_stale_owned_server() {
  if [[ -f "$web_pid_file" ]]; then
    local stale_pid
    stale_pid="$(cat "$web_pid_file" 2>/dev/null || true)"
    if [[ -n "$stale_pid" ]] && kill -0 "$stale_pid" 2>/dev/null; then
      echo "[demo] stopping stale owned web server pid $stale_pid" >&2
      cleanup_web_server
    else
      rm -f "$web_pid_file"
    fi
  fi
}

start_web_server() {
  require_command curl

  check_stale_owned_server
  stop_port_listener "$web_port" "web"

  : >"$web_log"
  printf "[demo] building web frontend... (0/? crates, 0s)" >&2
  ./scripts/web/serve-static.sh "$web_port" >"$web_log" 2>&1 &
  web_server_pid=$!
  echo "$web_server_pid" >"$web_pid_file"

  local ready="0"
  local elapsed=0
  local total="?"
  for _ in $(seq 1 600); do
    if ! kill -0 "$web_server_pid" 2>/dev/null; then
      echo "" >&2
      echo "[demo] static web server exited before becoming reachable" >&2
      tail -n 200 "$web_log" >&2 || true
      exit 1
    fi
    if curl -fsS "$web_url" >/dev/null 2>&1; then
      ready="1"
      break
    fi
    sleep 1
    elapsed=$((elapsed + 1))
    if (( elapsed % 5 == 0 )); then
      local progress
      progress="$(grep -c 'INFO Compiled' "$web_log" 2>/dev/null || echo 0)"
      if [[ "$total" == "?" ]]; then
        local last_entry
        last_entry="$(grep -o 'Compiled \[[0-9]*/[0-9]*\]' "$web_log" 2>/dev/null | tail -1 | grep -o '/[0-9]*' | tr -d '/' || true)"
        [[ -n "$last_entry" ]] && total="$last_entry"
      fi
      printf "\r[demo] building web frontend... (%s/%s crates, %ss)" "$progress" "$total" "$elapsed" >&2
    fi
  done

  if [[ "$ready" != "1" ]]; then
    echo "" >&2
    echo "[demo] timed out waiting for static web server at $web_url" >&2
    tail -n 200 "$web_log" >&2 || true
    exit 1
  fi
  printf "\r[demo] web server ready at %s%s\n" "$web_url" "$(printf ' %.0s' {1..30})" >&2
}

set_manual_browser_command() {
  if [[ "$OSTYPE" == darwin* ]]; then
    manual_browser_cmd="open -na \"Google Chrome\" --args --user-data-dir=\"$web_profile_dir\" --no-first-run --new-window \"$web_url\""
  else
    manual_browser_cmd="google-chrome --user-data-dir=\"$web_profile_dir\" --no-first-run --new-window \"$web_url\""
  fi
}

select_browser() {
  set_manual_browser_command

  if [[ "$browser" == "none" ]]; then
    return 0
  fi

  if [[ "$OSTYPE" == darwin* ]]; then
    if [[ "$browser" == "auto" || "$browser" == "chrome" ]]; then
      if open -Ra "Google Chrome" >/dev/null 2>&1; then
        browser_display_name="Google Chrome"
        manual_browser_cmd="open -na \"Google Chrome\" --args --user-data-dir=\"$web_profile_dir\" --no-first-run --new-window \"$web_url\""
        return 0
      fi
    fi
    if [[ "$browser" == "auto" || "$browser" == "chromium" ]]; then
      if open -Ra "Chromium" >/dev/null 2>&1; then
        browser_display_name="Chromium"
        manual_browser_cmd="open -na \"Chromium\" --args --user-data-dir=\"$web_profile_dir\" --no-first-run --new-window \"$web_url\""
        return 0
      fi
    fi
    return 1
  fi

  local candidates=()
  if [[ "$browser" == "auto" || "$browser" == "chrome" ]]; then
    candidates+=(google-chrome google-chrome-stable chrome)
  fi
  if [[ "$browser" == "auto" || "$browser" == "chromium" ]]; then
    candidates+=(chromium chromium-browser)
  fi

  local candidate=""
  for candidate in "${candidates[@]}"; do
    if command -v "$candidate" >/dev/null 2>&1; then
      browser_display_name="$candidate"
      manual_browser_cmd="$candidate --user-data-dir=\"$web_profile_dir\" --no-first-run --new-window \"$web_url\""
      return 0
    fi
  done

  return 1
}

launch_browser() {
  : >"$browser_log"

  if [[ "$browser" == "none" ]]; then
    echo "[demo] browser auto-launch skipped (--browser none)"
    echo "[demo] manual browser launch:"
    echo "  $manual_browser_cmd"
    return 0
  fi

  if ! select_browser; then
    echo "[demo] no Chrome/Chromium found; opening default browser" >&2
    if [[ "$OSTYPE" == darwin* ]]; then
      open "$web_url" >>"$browser_log" 2>&1 || true
    elif command -v xdg-open >/dev/null 2>&1; then
      xdg-open "$web_url" >>"$browser_log" 2>&1 || true
    else
      echo "[demo] no browser opener found; open manually: $web_url" >&2
    fi
    return 0
  fi

  if [[ "$OSTYPE" == darwin* ]]; then
    if ! eval "$manual_browser_cmd" >>"$browser_log" 2>&1; then
      echo "[demo] browser launch failed for $browser_display_name" >&2
      tail -n 200 "$browser_log" >&2 || true
      exit 1
    fi
  else
    if ! bash -lc "$manual_browser_cmd" >>"$browser_log" 2>&1 & then
      echo "[demo] browser launch failed for $browser_display_name" >&2
      tail -n 200 "$browser_log" >&2 || true
      exit 1
    fi
    local browser_pid=$!
    sleep 1
    if ! kill -0 "$browser_pid" 2>/dev/null; then
      echo "[demo] browser launch process exited immediately for $browser_display_name" >&2
      tail -n 200 "$browser_log" >&2 || true
      exit 1
    fi
  fi
}

print_runtime_summary() {
  cat <<EOF
[demo] mode: $mode
[demo] reset: $reset
[demo] web url: $web_url
[demo] web port: $web_port
[demo] web log: $web_log
[demo] browser profile: $web_profile_dir
[demo] browser log: $browser_log
[demo] tui data dir: $tui_data_dir
[demo] tui bind address: $tui_bind_address
[demo] tui device id: $tui_device_id
[demo] metadata: $meta_file
[demo] note: browser-direct websocket transport is runtime-assigned and will appear in exported invitation or enrollment codes
EOF
}

check_tui_prereqs() {
  if [[ ! -x "$repo_root/bin/aura" ]]; then
    echo "[demo] expected built binary at ./bin/aura; run 'just build-dev' first" >&2
    exit 1
  fi

  local tui_port="${tui_bind_address##*:}"
  if [[ "$tui_port" =~ ^[0-9]+$ ]]; then
    stop_port_listener "$tui_port" "tui"
  fi
}

run_tui() {
  check_tui_prereqs
  "$repo_root/bin/aura" tui \
    --data-dir "$tui_data_dir" \
    --device-id "$tui_device_id" \
    --bind-address "$tui_bind_address"
}

run_web_wait_loop() {
  echo "[demo] web-only mode is active; press Ctrl+C to stop"
  while true; do
    if [[ -n "$web_server_pid" ]] && ! kill -0 "$web_server_pid" 2>/dev/null; then
      echo "[demo] static web server exited unexpectedly" >&2
      tail -n 200 "$web_log" >&2 || true
      exit 1
    fi
    sleep 2
  done
}

prepare_dirs
write_metadata

case "$mode" in
  dual)
    start_web_server
    select_browser
    write_metadata
    launch_browser
    print_runtime_summary
    run_tui
    ;;
  tui)
    write_metadata
    print_runtime_summary
    run_tui
    ;;
  web)
    start_web_server
    select_browser
    write_metadata
    launch_browser
    print_runtime_summary
    run_web_wait_loop
    ;;
esac
