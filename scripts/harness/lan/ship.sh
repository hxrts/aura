#!/usr/bin/env bash
# Build the LAN run artifacts once on this host and ship them to the remote
# host instead of rebuilding there. Both hosts must be the same OS/arch
# (checked) and the remote checkout is moved to this host's clean commit.
#
# Usage: AURA_E2E_REMOTE=user@host scripts/harness/lan/ship.sh [--no-build]
#
# Shipped: bin/aura, target/release/tool_repl, the web release bundle and the
# untracked tailwind.css it links to. Shipped files are touched after the
# remote checkout moves so the web freshness check sees them as current.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=env.sh
. "$here/env.sh"
[ -n "${AURA_E2E_REMOTE:-}" ] || { echo 'set AURA_E2E_REMOTE to the other host (user@host)' >&2; exit 2; }
build=1
[ "${1:-}" = --no-build ] && build=0

cd "$AURA_E2E_ROOT"
[ -z "$(git status --porcelain --untracked-files=no)" ] || { echo 'ship: checkout has uncommitted changes' >&2; exit 1; }
commit="$(git rev-parse HEAD)"

local_platform="$(uname -sm)"
remote_platform="$(ssh -o BatchMode=yes "$AURA_E2E_REMOTE" 'uname -sm')"
[ "$local_platform" = "$remote_platform" ] || {
  echo "ship: platform mismatch: local $local_platform, remote $remote_platform" >&2; exit 1;
}

if [ "$build" = 1 ]; then
  for lane in terminal-live web-live; do
    AURA_EXPECT_COMMIT="$commit" nix develop -c bash "$here/build.sh" "$lane"
  done
  AURA_EXPECT_COMMIT="$commit" nix develop -c just e2e-build-harness-live
fi

# Agents may edit the tree while a long build runs; refuse to ship a mixed build.
[ -z "$(git status --porcelain --untracked-files=no)" ] && [ "$(git rev-parse HEAD)" = "$commit" ] || {
  echo "ship: checkout changed during the build; rebuild from a clean commit" >&2; exit 1;
}

web_public="target/dx/aura-web/release/web/public"
tailwind="crates/aura-web/public/assets/tailwind.css"
for f in bin/aura target/release/tool_repl "$web_public/index.html" "$tailwind"; do
  [ -e "$f" ] || { echo "ship: missing artifact $f" >&2; exit 1; }
done

# Every dynamic library outside the system must exist on the remote host.
for lib in $(otool -L bin/aura target/release/tool_repl | awk '/^\t\/nix\//{print $1}' | sort -u); do
  ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "test -e '$lib'" || {
    echo "ship: remote host lacks $lib" >&2; exit 1;
  }
done

remote_root="$AURA_E2E_REMOTE_ROOT"
ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "cd $remote_root && git fetch -q origin && \
  [ -z \"\$(git status --porcelain --untracked-files=no)\" ] && git checkout -q main && git merge -q --ff-only $commit && \
  mkdir -p bin target/release $web_public crates/aura-web/public/assets"

rsync -a bin/aura "$AURA_E2E_REMOTE:$remote_root/bin/aura"
rsync -a target/release/tool_repl "$AURA_E2E_REMOTE:$remote_root/target/release/tool_repl"
rsync -a "$tailwind" "$AURA_E2E_REMOTE:$remote_root/$tailwind"
rsync -a --delete --links "$web_public/" "$AURA_E2E_REMOTE:$remote_root/$web_public/"

ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "cd $remote_root && \
  find bin/aura target/release/tool_repl $web_public -exec touch {} + && \
  test \"\$(git rev-parse HEAD)\" = $commit"
echo "ship: $commit shipped to $AURA_E2E_REMOTE"
