#!/usr/bin/env bash
# Run browser smoke tests with web asset preparation and Playwright driver.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mkdir -p artifacts/harness/browser
log_file="$repo_root/artifacts/harness/browser/ci-browser.log"
exec > >(tee "$log_file") 2>&1

web_tools_cache_root="$repo_root/target/aura-web-tools-ci"

ensure_browser_build_space() {
  local min_free_kb=$((8 * 1024 * 1024))
  local free_kb
  free_kb="$(df -Pk "$repo_root" | awk 'NR==2 { print $4 }')"

  if [ -z "$free_kb" ] || [ "$free_kb" -ge "$min_free_kb" ]; then
    return 0
  fi

  echo "[browser-smoke] low disk headroom before web build; pruning completed build outputs" >&2
  rm -rf \
    "$repo_root/target/tests" \
    "$repo_root/target/kani" \
    "$repo_root/target/release" \
    "$repo_root/target/debug/incremental" \
    "$repo_root/target/debug/examples" \
    "$repo_root/target/wasm-release" \
    "$repo_root/target/wasm32-unknown-unknown" \
    "$repo_root/target/dx" \
    "$web_tools_cache_root"

  df -h "$repo_root" >&2 || true
}

run_dx_build() {
  local attempt=1
  local max_attempts=2

  while true; do
    mkdir -p \
      "$repo_root/target/dx/aura-web/release/web/public/assets" \
      "$repo_root/target/dx/aura-web/release/web/public/fonts"
    if NO_COLOR=true ../../scripts/web/dx.sh build --release --platform web --package aura-web --bin aura-web --features web >/dev/null; then
      return 0
    fi
    if [ "$attempt" -ge "$max_attempts" ]; then
      return 1
    fi
    echo "[browser-smoke] dx build failed; recreating output directories and retrying once" >&2
    attempt=$((attempt + 1))
  done
}

prepare_browser_web_assets() {
  (
    cd crates/aura-web
    if [ ! -d node_modules ] || [ ! -d node_modules/ws ] || [ ! -x node_modules/.bin/esbuild ] || [ ! -x node_modules/.bin/tailwindcss ]; then
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
  npm run build
  npm run install-browsers
)

cargo run --quiet --manifest-path toolkit/xtask/Cargo.toml -- check browser-install
export AURA_HARNESS_WEB_BUILD_PROFILE=release
export AURA_HARNESS_WEB_SERVER_READY_TIMEOUT_SECS=1800
export AURA_WEB_TOOLS_CACHE_ROOT="$web_tools_cache_root"

ensure_browser_build_space
prepare_browser_web_assets

cargo run -p aura-harness --bin aura-harness -- run \
  --config configs/harness/browser-loopback.toml \
  --scenario scenarios/harness/semantic-observation-browser-smoke.toml \
  --artifacts-dir artifacts/harness/browser
