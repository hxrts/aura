#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mkdir -p artifacts/harness/browser

(
  cd crates/aura-harness/playwright-driver
  npm ci
  npm run install-browsers
)

bash scripts/check/harness-browser-install.sh

cargo build -p aura-harness --bin aura-harness -q
export AURA_HARNESS_BIN="$repo_root/target/debug/aura-harness"
export AURA_HARNESS_WEB_BUILD_PROFILE=debug

./scripts/web/serve-static.sh 4173 > artifacts/harness/browser/web-serve.log 2>&1 &
server_pid=$!
echo "$server_pid" > artifacts/harness/browser/web-serve.pid

cleanup() {
  if [ -f artifacts/harness/browser/web-serve.pid ]; then
    kill "$(cat artifacts/harness/browser/web-serve.pid)" 2>/dev/null || true
  fi
}
trap cleanup EXIT

web_ready=0
for _ in $(seq 1 300); do
  if [ -f artifacts/harness/browser/web-serve.pid ]; then
    if ! kill -0 "$(cat artifacts/harness/browser/web-serve.pid)" 2>/dev/null; then
      echo "static web server exited before becoming reachable"
      tail -n 200 artifacts/harness/browser/web-serve.log || true
      exit 1
    fi
  fi
  if curl -fsS "http://127.0.0.1:4173/" >/dev/null 2>&1; then
    web_ready=1
    break
  fi
  sleep 1
done

if [ "$web_ready" -ne 1 ]; then
  echo "timed out waiting for static web server at http://127.0.0.1:4173/"
  tail -n 200 artifacts/harness/browser/web-serve.log || true
  exit 1
fi

bash scripts/harness/run-matrix.sh --lane web "$@"
