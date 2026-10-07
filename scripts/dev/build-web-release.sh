#!/usr/bin/env bash
# Build the same release/harness web bundle used by the LAN static server.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
web_root="$repo_root/crates/aura-web"
cd "$web_root"
if [[ ! -d node_modules || ! -d node_modules/ws ]]; then
  npm ci
fi
npm run tailwind:build
NO_COLOR=true ../../scripts/web/dx.sh build --release --profile wasm --platform web --package aura-web --bin aura-web --features web,harness

public_dir="$repo_root/target/dx/aura-web/release/web/public"
[[ -f "$public_dir/index.html" ]] || { echo 'web release build produced no index.html' >&2; exit 1; }
mkdir -p "$public_dir/assets"
ln -sfn "$web_root/public/assets/tailwind.css" "$public_dir/assets/tailwind.css"
printf 'Web bundle: %s\n' "$public_dir"
