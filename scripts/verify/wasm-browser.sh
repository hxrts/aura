#!/usr/bin/env bash
# Prepare the selected real browser, then use the canonical lock-matched runner.
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
browser="${AURA_TEST_CHROMIUM:-}"
driver="${AURA_TEST_CHROMEDRIVER:-}"
[[ -n "$browser" && -x "$browser" && -n "$driver" && -x "$driver" ]] || {
  echo 'wasm-browser: selected browser and WebDriver executables are required' >&2
  exit 1
}
version_build() {
  local output
  output="$("$1" --version)" || return $?
  if [[ "$output" =~ ([0-9]+\.[0-9]+\.[0-9]+)\.[0-9]+ ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
  else
    echo 'wasm-browser: executable did not report a browser build version' >&2
    return 1
  fi
}
browser_build="$(version_build "$browser")" || exit 1
driver_build="$(version_build "$driver")" || exit 1
[[ "$browser_build" == "$driver_build" ]] || {
  echo 'wasm-browser: selected browser and WebDriver builds differ' >&2
  exit 1
}
capabilities="$(mktemp "${TMPDIR:-/tmp}/aura-webdriver.XXXXXXXX")"
trap 'rm -f "$capabilities"' EXIT
jq -n --arg binary "$browser" '{"goog:chromeOptions": {binary: $binary}}' > "$capabilities"
export CHROMEDRIVER="$driver"
export WASM_BINDGEN_TEST_WEBDRIVER_JSON="$capabilities"
export WASM_BINDGEN_USE_BROWSER=1
"$script_dir/wasm-bindgen.sh" "$@"
