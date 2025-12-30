#!/usr/bin/env bash
set -euo pipefail

# Ensure demo mode always runs with simulated peers (Alice/Carol).
args=("$@")
for arg in "${args[@]}"; do
  if [[ "$arg" == "--demo" ]]; then
    exec cargo run -p aura-terminal --features development -- "${args[@]}"
  fi
done

# Non-demo path: prefer prebuilt binary if present, otherwise cargo run.
if [[ -x "$(pwd)/target/release/aura" ]]; then
  exec "$(pwd)/target/release/aura" "${args[@]}"
fi

exec cargo run -p aura-terminal -- "${args[@]}"
