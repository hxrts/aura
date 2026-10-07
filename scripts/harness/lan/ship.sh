#!/usr/bin/env bash
# Build the LAN run artifacts once on this host and ship them to the remote
# host instead of rebuilding there. Both hosts must be the same OS/arch
# (checked) and the remote checkout is moved to this host's clean commit.
#
# Usage: AURA_E2E_REMOTE=user@host scripts/harness/lan/ship.sh [--no-build]
#
# The terminal `aura` and harness `tool_repl` are built hermetically from the
# committed tree with crate2nix (`nix build .#aura-lan-terminal
# .#aura-lan-harness`): each crate is its own derivation, so unchanged crates
# are reused across worktrees and hosts. Their closures go to the remote with
# `nix copy`, which also carries every runtime library they link. The web
# release bundle stays on dx (build.sh web-live) and is rsynced with the
# untracked tailwind.css it links to. Installed files are touched after the
# remote checkout moves so the web freshness check sees them as current.
#
# `nix copy` needs the remote user to be a Nix trusted-user (or a signing key
# the remote trusts). AURA_NIX_REMOTE_PROGRAM overrides the remote daemon path
# for non-login ssh shells without nix on PATH.
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

nix_bin="${AURA_NIX_BIN:-$(command -v nix || printf '/nix/var/nix/profiles/default/bin/nix')}"
links=.nix-ship
terminal_link="$links/aura-lan-terminal"
harness_link="$links/aura-lan-harness"
if [ "$build" = 1 ]; then
  mkdir -p "$links"
  # The out-links are GC roots: `just nix-store-gc --apply` keeps the shipped build.
  "$nix_bin" build ".#aura-lan-terminal" --out-link "$terminal_link"
  "$nix_bin" build ".#aura-lan-harness" --out-link "$harness_link"
  AURA_EXPECT_COMMIT="$commit" "$nix_bin" develop -c bash "$here/build.sh" web-live
fi

# Agents may edit the tree while a long build runs; refuse to ship a mixed build.
[ -z "$(git status --porcelain --untracked-files=no)" ] && [ "$(git rev-parse HEAD)" = "$commit" ] || {
  echo "ship: checkout changed during the build; rebuild from a clean commit" >&2; exit 1;
}

terminal_out="$(readlink "$terminal_link" || true)"
harness_out="$(readlink "$harness_link" || true)"
web_public="target/dx/aura-web/release/web/public"
tailwind="crates/aura-web/public/assets/tailwind.css"
for f in "$terminal_out/bin/aura" "$harness_out/bin/tool_repl" "$web_public/index.html" "$tailwind"; do
  [ -e "$f" ] || { echo "ship: missing artifact $f (run without --no-build)" >&2; exit 1; }
done

remote_program="${AURA_NIX_REMOTE_PROGRAM:-/nix/var/nix/profiles/default/bin/nix-daemon}"
"$nix_bin" copy --no-check-sigs --to "ssh-ng://$AURA_E2E_REMOTE?remote-program=$remote_program" \
  "$terminal_out" "$harness_out"

remote_root="$AURA_E2E_REMOTE_ROOT"
ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "cd $remote_root && git fetch -q origin && \
  [ -z \"\$(git status --porcelain --untracked-files=no)\" ] && git checkout -q main && git merge -q --ff-only $commit && \
  mkdir -p bin target/release $web_public crates/aura-web/public/assets && \
  install -m 0755 '$terminal_out/bin/aura' bin/aura && \
  install -m 0755 '$harness_out/bin/tool_repl' target/release/tool_repl"

rsync -a "$tailwind" "$AURA_E2E_REMOTE:$remote_root/$tailwind"
rsync -a --delete --links "$web_public/" "$AURA_E2E_REMOTE:$remote_root/$web_public/"

ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "cd $remote_root && \
  find bin/aura target/release/tool_repl $web_public -exec touch {} + && \
  test \"\$(git rev-parse HEAD)\" = $commit"

# Run the same binaries locally as on the remote host.
mkdir -p bin target/release
install -m 0755 "$terminal_out/bin/aura" bin/aura
install -m 0755 "$harness_out/bin/tool_repl" target/release/tool_repl
echo "ship: $commit shipped to $AURA_E2E_REMOTE ($terminal_out, $harness_out)"
