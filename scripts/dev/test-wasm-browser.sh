#!/usr/bin/env bash
set -euo pipefail
source_script="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/scripts/verify/wasm-browser.sh}"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-browser-test.XXXXXXXX")"
trap 'rm -rf "$fixture"' EXIT
cp "$source_script" "$fixture/wasm-browser.sh"
cat > "$fixture/browser" <<'EOF'
#!/usr/bin/env bash
printf 'Chromium 154.0.8037.99\n'
EOF
cat > "$fixture/driver" <<'EOF'
#!/usr/bin/env bash
printf 'ChromeDriver %s\n' "${DRIVER_VERSION:-154.0.8037.92}"
EOF
cat > "$fixture/wasm-bindgen.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == actual.wasm && "$2" == --nocapture ]]
[[ "$CHROMEDRIVER" == "$AURA_TEST_CHROMEDRIVER" && "$WASM_BINDGEN_USE_BROWSER" == 1 ]]
[[ "$(jq -r '."goog:chromeOptions".binary' "$WASM_BINDGEN_TEST_WEBDRIVER_JSON")" == "$AURA_TEST_CHROMIUM" ]]
printf '%s\n' "$WASM_BINDGEN_TEST_WEBDRIVER_JSON" > "$RECORD"
exit "${RUNNER_STATUS:-0}"
EOF
chmod +x "$fixture/browser" "$fixture/driver" "$fixture/wasm-bindgen.sh"
export AURA_TEST_CHROMIUM="$fixture/browser" AURA_TEST_CHROMEDRIVER="$fixture/driver" RECORD="$fixture/record"
for status in 0 7; do
 actual=0
 RUNNER_STATUS="$status" bash "$fixture/wasm-browser.sh" actual.wasm --nocapture || actual=$?
 [[ "$actual" == "$status" && ! -e "$(cat "$RECORD")" ]]
done
rm "$RECORD"
if DRIVER_VERSION=155.0.1.1 bash "$fixture/wasm-browser.sh" actual.wasm --nocapture; then exit 1; fi
[[ ! -e "$RECORD" ]]
if AURA_TEST_CHROMIUM= bash "$fixture/wasm-browser.sh" actual.wasm --nocapture; then exit 1; fi
[[ ! -e "$RECORD" ]]
cat > "$fixture/malformed" <<'EOF'
#!/usr/bin/env bash
printf 'not-a-version\n'
EOF
cat > "$fixture/failed-probe" <<'EOF'
#!/usr/bin/env bash
printf 'Chromium 154.0.8037.99\n'
exit 9
EOF
chmod +x "$fixture/malformed" "$fixture/failed-probe"
for invalid in malformed failed-probe; do
 if AURA_TEST_CHROMIUM="$fixture/$invalid" AURA_TEST_CHROMEDRIVER="$fixture/$invalid" bash "$fixture/wasm-browser.sh" actual.wasm --nocapture; then exit 1; fi
 [[ ! -e "$RECORD" ]]
done
printf 'browser wrapper success/failure/mismatch/missing fixtures PASS\n'
