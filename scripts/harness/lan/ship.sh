#!/usr/bin/env bash
# Build the LAN run artifacts once on this host and ship them to the remote
# host instead of rebuilding there. Both hosts must be the same OS/arch
# (checked) and the remote checkout is moved to this host's clean commit.
#
# Usage: AURA_E2E_REMOTE=user@host scripts/harness/lan/ship.sh [--no-build]
#
# `aura` and `tool_repl` are built with Cargo's `lan` profile (release
# optimization without whole-program LTO) in this checkout's target/, through
# the build budget, so unchanged crates come from target/ and sccache and a
# one-crate change rebuilds only its dependents (work/8.md Task 197). The
# binaries go to the remote with rsync; the Nix store paths they reference at
# run time (dynamic libraries from the dev shell) go with `nix copy` and are
# rooted on both hosts under .nix-ship/, together with the dev shell itself,
# so `just nix-store-gc --apply` keeps them (Task 194). The web release
# bundle stays on dx (build.sh web-live) and is rsynced with the untracked
# tailwind.css it links to. Installed files are touched after the remote
# checkout moves so the web freshness check sees them as current.
#
# `nix copy` needs the remote user to be a Nix trusted-user (or a signing key
# the remote trusts). AURA_NIX_REMOTE_PROGRAM overrides the remote daemon path
# and AURA_NIX_REMOTE_BIN the remote nix-store directory, for non-login ssh
# shells without nix on PATH.
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

remote_root="$AURA_E2E_REMOTE_ROOT"
# A previous run's harness (tool_repl and the aura processes it drives) is a
# build consumer the budget waits on, and its binaries are about to be
# replaced: stop it on both hosts first (work/8.md Task 209).
bash "$here/drv.sh" stop
ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "cd $remote_root && bash scripts/harness/lan/drv.sh stop"
# Wait for the volume, the lock and other worktrees' builds instead of failing.
export AURA_BUILD_WAIT_SECONDS="${AURA_BUILD_WAIT_SECONDS:-3600}"

nix_bin="${AURA_NIX_BIN:-$(command -v nix || printf '/nix/var/nix/profiles/default/bin/nix')}"
links=.nix-ship
aura_bin=target/lan/aura
repl_bin=target/lan/tool_repl
if [ "$build" = 1 ]; then
  mkdir -p "$links"
  # The dev-shell profile is a GC root: the toolchain survives nix-store-gc.
  in_shell=("$nix_bin" develop --profile "$links/devshell" -c)
  "${in_shell[@]}" bash scripts/dev/build-budget.sh --lane lan-ship -- \
    cargo build --profile lan -p aura-terminal --bin aura --no-default-features --features terminal
  "${in_shell[@]}" bash scripts/dev/build-budget.sh --lane lan-ship -- \
    cargo build --profile lan -p aura-harness --bin tool_repl
  AURA_EXPECT_COMMIT="$commit" "${in_shell[@]}" bash "$here/build.sh" web-live
fi

# Agents may edit the tree while a long build runs; refuse to ship a mixed build.
[ -z "$(git status --porcelain --untracked-files=no)" ] && [ "$(git rev-parse HEAD)" = "$commit" ] || {
  echo "ship: checkout changed during the build; rebuild from a clean commit" >&2; exit 1;
}

web_public="target/dx/aura-web/release/web/public"
tailwind="crates/aura-web/public/assets/tailwind.css"
for f in "$aura_bin" "$repl_bin" "$web_public/index.html" "$tailwind"; do
  [ -e "$f" ] || { echo "ship: missing artifact $f (run without --no-build)" >&2; exit 1; }
done

# The run-time closure: Nix store paths the binaries reference.
runtime_refs=()
while IFS= read -r ref; do
  [ -e "$ref" ] && runtime_refs+=("$ref")
done < <(bash "$here/runtime-refs.sh" "$aura_bin" "$repl_bin")

remote_bin="${AURA_NIX_REMOTE_BIN:-/nix/var/nix/profiles/default/bin}"
rm -f "$links"/runtime-*
root_cmds=''
i=0
for ref in "${runtime_refs[@]}"; do
  "$(dirname "$nix_bin")/nix-store" --add-root "$links/runtime-$i" -r "$ref" >/dev/null
  root_cmds+="$remote_bin/nix-store --add-root .nix-ship/runtime-$i -r '$ref' >/dev/null && "
  i=$((i + 1))
done
if [ "${#runtime_refs[@]}" -gt 0 ]; then
  remote_program="${AURA_NIX_REMOTE_PROGRAM:-/nix/var/nix/profiles/default/bin/nix-daemon}"
  "$nix_bin" copy --no-check-sigs --to "ssh-ng://$AURA_E2E_REMOTE?remote-program=$remote_program" \
    "${runtime_refs[@]}"
fi

ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "cd $remote_root && git fetch -q origin && \
  [ -z \"\$(git status --porcelain --untracked-files=no)\" ] && git checkout -q main && git merge -q --ff-only $commit && \
  mkdir -p bin target/release $web_public crates/aura-web/public/assets .nix-ship && \
  rm -f .nix-ship/runtime-* && ${root_cmds}true"
rsync -a "$aura_bin" "$AURA_E2E_REMOTE:$remote_root/bin/aura"
rsync -a "$repl_bin" "$AURA_E2E_REMOTE:$remote_root/target/release/tool_repl"
rsync -a "$tailwind" "$AURA_E2E_REMOTE:$remote_root/$tailwind"
rsync -a --delete --links "$web_public/" "$AURA_E2E_REMOTE:$remote_root/$web_public/"

ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "cd $remote_root && \
  find bin/aura target/release/tool_repl $web_public -exec touch {} + && \
  test \"\$(git rev-parse HEAD)\" = $commit"

# Run the same binaries locally as on the remote host.
mkdir -p bin target/release
install -m 0755 "$aura_bin" bin/aura
install -m 0755 "$repl_bin" target/release/tool_repl
echo "ship: $commit shipped to $AURA_E2E_REMOTE (${#runtime_refs[@]} runtime store paths)"
