#!/usr/bin/env bash
# One guarded build entry point for each host of the LAN harness.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
lane="${1:-}"
mode="${2:-}"
case "$lane" in
  terminal) recipe=e2e-build-terminal ;;
  terminal-live) recipe=e2e-build-terminal-live ;;
  terminal-dev) recipe=e2e-build-terminal-dev ;;
  web) recipe=e2e-build-web ;;
  web-live) recipe=e2e-build-web-live ;;
  harness) recipe=e2e-build-harness ;;
  harness-live) recipe=e2e-build-harness-live ;;
  *) echo 'usage: scripts/harness/lan/build.sh terminal|terminal-live|terminal-dev|web|web-live|harness|harness-live [--dry-run]' >&2; exit 2 ;;
esac
[[ -z "$mode" || "$mode" == --dry-run ]] || { echo 'only --dry-run is accepted after the lane' >&2; exit 2; }

cd "$repo_root"
commit="$(git rev-parse HEAD)"
if [[ -n "${AURA_EXPECT_COMMIT:-}" && "$commit" != "$AURA_EXPECT_COMMIT" ]]; then
  echo "LAN build: expected commit $AURA_EXPECT_COMMIT, found $commit" >&2
  exit 1
fi
printf 'LAN build: host=%s commit=%s lane=%s recipe=%s\n' "$(hostname)" "$commit" "$lane" "$recipe"
if [[ -n "$(git status --porcelain)" ]]; then
  if [[ -n "${AURA_EXPECT_COMMIT:-}" ]]; then
    echo 'LAN build: fixed-commit validation requires a clean checkout' >&2
    exit 1
  fi
  echo 'LAN build: checkout has uncommitted changes; this is not fixed-commit validation' >&2
fi
[[ "$mode" != --dry-run ]] || { printf 'Dry run: just %s\n' "$recipe"; exit 0; }

if [[ -n "${IN_NIX_SHELL:-}" ]]; then
  exec just "$recipe"
fi
nix_bin="${AURA_NIX_BIN:-$(command -v nix || printf '/nix/var/nix/profiles/default/bin/nix')}"
exec "$nix_bin" develop -c just "$recipe"
