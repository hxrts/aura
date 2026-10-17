#!/usr/bin/env bash
set -euo pipefail
repo_root=$(cd "$(dirname "$0")/../.." && pwd -P)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-shell-syntax.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
printf 'true\n' > "$fixture/first.sh"
printf 'if true; then\n' > "$fixture/later-broken.sh"
if bash "$repo_root/scripts/dev/check-shell-syntax.sh" "$fixture/first.sh" "$fixture/later-broken.sh" > "$fixture/log" 2>&1; then
  echo 'later malformed shell file was not rejected' >&2; exit 1
fi
printf '  true\nfi\n' >> "$fixture/later-broken.sh"
bash "$repo_root/scripts/dev/check-shell-syntax.sh" "$fixture/first.sh" "$fixture/later-broken.sh"
echo 'all expanded shell inputs checked'
