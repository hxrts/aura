#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-browser-cache-owner: $*" >&2
  exit 1
}

driver="crates/aura-harness/playwright-driver/src/playwright_driver.ts"
start_line="$(rg -n '^function resetUiObservationState' "$driver" | cut -d: -f1)"
end_line="$(rg -n '^function resetObservationState' "$driver" | cut -d: -f1)"

if [ -z "$start_line" ] || [ -z "$end_line" ]; then
  fail "could not locate browser cache owner function boundaries"
fi

hits="$(rg --no-heading -n \
  'session\.uiStateCache = null|session\.uiStateCacheJson = null|session\.uiStateVersion = 0|session\.requiredUiStateRevision = 0' \
  "$driver" || true)"

while IFS= read -r hit; do
  [ -z "$hit" ] && continue
  line="${hit%%:*}"
  if [ "$line" -lt "$start_line" ] || [ "$line" -ge "$end_line" ]; then
    echo "$hit" >&2
    fail "browser cache reset logic must stay inside resetUiObservationState"
  fi
done <<< "$hits"

echo "harness browser cache owner: clean"
