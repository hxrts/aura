#!/usr/bin/env bash
# Ensure node, npm, TypeScript, and Playwright are installed for browser tests.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
driver_dir="$repo_root/crates/aura-harness/playwright-driver"

fail() {
  echo "harness-browser-toolchain: $*" >&2
  exit 1
}

command -v node >/dev/null 2>&1 || fail "node not found in PATH"
command -v npm >/dev/null 2>&1 || fail "npm not found in PATH"

compiler_path="$driver_dir/node_modules/typescript/bin/tsc"
playwright_path="$driver_dir/node_modules/playwright/package.json"

if [[ ! -x "$compiler_path" || ! -f "$playwright_path" ]]; then
  (
    cd "$driver_dir"
    npm ci
  )
fi

[[ -x "$compiler_path" ]] || fail "missing TypeScript compiler after npm ci: $compiler_path"
[[ -f "$playwright_path" ]] || fail "missing Playwright package after npm ci: $playwright_path"

echo "harness browser toolchain: clean"
