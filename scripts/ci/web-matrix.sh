#!/usr/bin/env bash
# Build web assets and run the browser harness matrix for a given suite.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mkdir -p artifacts/harness/browser
log_file="$repo_root/artifacts/harness/browser/ci-matrix-web.log"
: >"$log_file"
exec >>"$log_file" 2>&1

web_tools_cache_root="$repo_root/target/aura-web-tools-ci"

run_dx_build() {
  local attempt=1
  local max_attempts=2

  while true; do
    mkdir -p \
      "$repo_root/target/dx/aura-web/release/web/public/assets" \
      "$repo_root/target/dx/aura-web/release/web/public/fonts"
    if NO_COLOR=true ../../scripts/web/dx.sh build --release --platform web --package aura-web --bin aura-web --features web,harness >/dev/null; then
      return 0
    fi
    if [ "$attempt" -ge "$max_attempts" ]; then
      return 1
    fi
    echo "[web-matrix] dx build failed; recreating output directories and retrying once" >&2
    attempt=$((attempt + 1))
  done
}

prepare_browser_web_assets() {
  (
    cd crates/aura-web
    if [ ! -d node_modules ] || [ ! -d node_modules/ws ]; then
      npm ci
    fi
    npm run tailwind:build >/dev/null
    mkdir -p \
      "$repo_root/target/dx/aura-web/release/web/public/assets" \
      "$repo_root/target/dx/aura-web/release/web/public/fonts"
    target_css="$repo_root/target/dx/aura-web/release/web/public/assets/tailwind.css"
    rm -f "$target_css"
    ln -s "$repo_root/crates/aura-web/public/assets/tailwind.css" "$target_css"
    run_dx_build
  )
}

rm -rf "$web_tools_cache_root"
mkdir -p "$web_tools_cache_root"

(
  cd crates/aura-harness/playwright-driver
  npm ci
  npm run install-browsers
)

cargo run --quiet --manifest-path toolkit/xtask/Cargo.toml -- check browser-install

cargo build -p aura-harness --bin aura-harness -q
export AURA_HARNESS_BIN="$repo_root/target/debug/aura-harness"
export AURA_HARNESS_WEB_BUILD_PROFILE=release
export AURA_HARNESS_WEB_SERVER_READY_TIMEOUT_SECS=1800
export AURA_WEB_TOOLS_CACHE_ROOT="$web_tools_cache_root"
export AURA_HARNESS_MATRIX_LOG_FILE="$repo_root/artifacts/harness/browser/matrix-web.log"

prepare_browser_web_assets

bash scripts/harness/run-matrix.sh --lane web "$@"
