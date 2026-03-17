#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-raw-backend-quarantine: $*" >&2
  exit 1
}

raw_impls=()
while IFS= read -r path; do
  [[ -n "$path" ]] || continue
  raw_impls+=("$path")
done < <(rg -l 'impl RawUiBackend for' crates/aura-harness/src/backend)
expected=(
  'crates/aura-harness/src/backend/local_pty.rs'
  'crates/aura-harness/src/backend/playwright_browser.rs'
)

if [[ "${#raw_impls[@]}" -ne "${#expected[@]}" ]]; then
  fail "expected exactly ${#expected[@]} raw backend impls, found ${#raw_impls[@]}: ${raw_impls[*]:-<none>}"
fi

for path in "${expected[@]}"; do
  printf '%s
' "${raw_impls[@]}" | grep -Fx "$path" >/dev/null \
    || fail "raw backend impl must stay quarantined to $path"
done

raw_accessors=()
while IFS= read -r path; do
  [[ -n "$path" ]] || continue
  raw_accessors+=("$path")
done < <(rg -l 'as_raw_ui_mut\(' crates/aura-harness/src)
for path in "${raw_accessors[@]}"; do
  case "$path" in
    crates/aura-harness/src/backend/mod.rs|crates/aura-harness/src/coordinator.rs)
      ;;
    *) fail "raw backend accessor escaped quarantine via $path" ;;
  esac
done

echo "harness raw backend quarantine: clean"
