#!/usr/bin/env bash
set -euo pipefail

port="${1:-4173}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
selected_port="$port"

source "$script_dir/log-bootstrap.sh"
aura_web_redirect_logs "$repo_root" "$repo_root/artifacts/aura-web/serve-dev-${selected_port}.log"

if command -v lsof >/dev/null 2>&1; then
  mapfile -t existing_pids < <(lsof -PiTCP:"$selected_port" -sTCP:LISTEN -t 2>/dev/null || true)
  if [ "${#existing_pids[@]}" -gt 0 ]; then
    echo "Port $selected_port is already in use; stopping existing listener(s)." >&2
    lsof -nP -iTCP:"$selected_port" -sTCP:LISTEN >&2 || true
    kill "${existing_pids[@]}" 2>/dev/null || true
    for _ in $(seq 1 30); do
      if ! lsof -PiTCP:"$selected_port" -sTCP:LISTEN -t >/dev/null 2>&1; then
        break
      fi
      sleep 0.1
    done
    if lsof -PiTCP:"$selected_port" -sTCP:LISTEN -t >/dev/null 2>&1; then
      echo "Port $selected_port is still busy after SIGTERM; force stopping listener(s)." >&2
      kill -9 "${existing_pids[@]}" 2>/dev/null || true
      sleep 0.1
    fi
    if lsof -PiTCP:"$selected_port" -sTCP:LISTEN -t >/dev/null 2>&1; then
      echo "Failed to clear port $selected_port." >&2
      lsof -nP -iTCP:"$selected_port" -sTCP:LISTEN >&2 || true
      exit 1
    fi
  fi
fi

echo "Serving aura-web on http://127.0.0.1:$selected_port"
cd "$repo_root/crates/aura-web"
mkdir -p ../../artifacts
if [ ! -d node_modules ]; then
  npm ci
fi
npm run tailwind:build

target_css_dir="../../target/dx/aura-web/debug/web/public/assets"
source_css="$(pwd)/public/assets/tailwind.css"
sync_tailwind_link() {
  mkdir -p "$target_css_dir"
  rm -f "$target_css_dir/tailwind.css"
  ln -s "$source_css" "$target_css_dir/tailwind.css"
}
sync_tailwind_link
npm run tailwind:watch > ../../artifacts/aura-web-tailwind.log 2>&1 &
tailwind_pid=$!
while true; do
  sync_tailwind_link
  sleep 1
done &
tailwind_link_pid=$!
cleanup() {
  kill "$tailwind_pid" 2>/dev/null || true
  kill "$tailwind_link_pid" 2>/dev/null || true
  stty sane 2>/dev/null || true
}
trap cleanup EXIT INT TERM
NO_COLOR=true ../../scripts/web/dx.sh serve --web --package aura-web --bin aura-web --features web --addr 0.0.0.0 --port "$selected_port" --open false
stty sane 2>/dev/null || true
