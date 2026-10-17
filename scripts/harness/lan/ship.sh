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
# tailwind.css it links to. Web admission retains the original producer's
# protected evidence; file modification times do not attest build completion.
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

# Resolve all lifecycle tools through this clean candidate's pinned shell even
# when the canonical ship entry is invoked from an ordinary terminal/SSH shell.
if [[ -z "${IN_NIX_SHELL:-}" ]]; then
  nix_bin="${AURA_NIX_BIN:-$(command -v nix || printf '/nix/var/nix/profiles/default/bin/nix')}"
  exec "$nix_bin" develop "$AURA_E2E_ROOT" --command bash "$here/ship.sh" "$@"
fi
cd "$AURA_E2E_ROOT"
[ -z "$(git status --porcelain --untracked-files=no)" ] || { echo 'ship: checkout has uncommitted changes' >&2; exit 1; }
commit="$(git rev-parse HEAD)"

local_platform="$(uname -sm)"
remote_platform="$(ssh -o BatchMode=yes "$AURA_E2E_REMOTE" 'uname -sm')"
[ "$local_platform" = "$remote_platform" ] || {
  echo "ship: platform mismatch: local $local_platform, remote $remote_platform" >&2; exit 1;
}

remote_root="$AURA_E2E_REMOTE_ROOT"
# Quote remote paths once for legacy remote-shell argument handling. Pin this
# behavior for modern upload senders too; older rsync ignores these variables.
# Do not reuse old-args for downloads: it disables a remote-sender file-list check.
ship_remote_destination() {
  local quoted
  printf -v quoted '%q' "$1"
  printf '%s:%s\n' "$AURA_E2E_REMOTE" "$quoted"
}
ship_remote_rsync() {
  RSYNC_OLD_ARGS=1 RSYNC_PROTECT_ARGS=0 rsync "$@"
}
# Bootstrap the exact candidate's portable lock closure before stopping an old
# checkout: older Darwin hosts have neither lockf nor flock. No remote build.
nix_bin="${AURA_NIX_BIN:-$(command -v nix || printf '/nix/var/nix/profiles/default/bin/nix')}"
remote_bin="${AURA_NIX_REMOTE_BIN:-/nix/var/nix/profiles/default/bin}"
remote_program="${AURA_NIX_REMOTE_PROGRAM:-/nix/var/nix/profiles/default/bin/nix-daemon}"
flock_bin="$(command -v flock)" || { echo 'ship: pinned portable flock is required' >&2; exit 127; }
flock_store="${flock_bin%/bin/flock}"
[[ "$flock_store" == /nix/store/* && -x "$flock_store/bin/flock" ]] || { echo 'ship: flock must come from the pinned Nix store' >&2; exit 1; }
mkdir -p .nix-ship
"$(dirname "$nix_bin")/nix-store" --add-root .nix-ship/lifecycle-flock -r "$flock_store" >/dev/null
"$nix_bin" copy --no-check-sigs --to "ssh-ng://$AURA_E2E_REMOTE?remote-program=$remote_program" "$flock_store"
printf -v remote_root_cmd '%q ' bash -c 'cd "$1" && mkdir -p .nix-ship && "$2/nix-store" --add-root .nix-ship/lifecycle-flock -r "$3" >/dev/null' bash "$remote_root" "$remote_bin" "$flock_store"
ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "$remote_root_cmd"
# Preserve old-checkout helpers/config/identity custody but execute the exact
# clean candidate driver, with its real remote $0 and the pinned tool on PATH.
bash "$here/drv.sh" stop
printf -v remote_stop_cmd '%q ' bash -c 'cd "$1"; root="$(pwd -P)"; exec "$2/nix" develop "$root" --command bash -c "$4" "$root/scripts/harness/lan/drv.sh" "$3" stop' bash "$remote_root" "$remote_bin" "$flock_store" 'export PATH="$1/bin:$PATH"; shift; source /dev/stdin'
ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "$remote_stop_cmd" < "$here/drv.sh"
# Wait for the volume, the lock and other worktrees' builds instead of failing.
export AURA_BUILD_WAIT_SECONDS="${AURA_BUILD_WAIT_SECONDS:-3600}"

nix_bin="${AURA_NIX_BIN:-$(command -v nix || printf '/nix/var/nix/profiles/default/bin/nix')}"
links=.nix-ship
aura_bin=target/lan/aura
repl_bin=target/lan/tool_repl
case "${AURA_E2E_LAN_HTTP_DEV:-0}" in
  0) terminal_features=terminal ;;
  1) terminal_features=terminal,lan-http-dev ;;
  *) echo 'ship: AURA_E2E_LAN_HTTP_DEV must be 0 or 1' >&2; exit 2 ;;
esac
if [ "$build" = 1 ]; then
  mkdir -p "$links"
  # The dev-shell profile is a GC root: the toolchain survives nix-store-gc.
  in_shell=("$nix_bin" develop --profile "$links/devshell" -c)
  "${in_shell[@]}" bash scripts/dev/build-budget.sh --lane lan-ship -- \
    cargo build --profile lan -p aura-terminal --bin aura --no-default-features --features "$terminal_features"
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

# Only the original producer may attest this build. A remote consumer retains
# these original bytes; it must never regenerate a successful build marker.
umask 077
web_transfer_evidence=$(mktemp "${TMPDIR:-/tmp}/aura-web-transfer.XXXXXX")
trap 'rm -f -- "$web_transfer_evidence"' EXIT
if ! bash "$AURA_E2E_ROOT/scripts/web/prebuilt-transfer.sh" export \
    "$AURA_E2E_ROOT" "$AURA_E2E_ROOT/$web_public" "$web_transfer_evidence"; then
  echo 'ship: web producer provenance does not match this clean candidate' >&2
  exit 1
fi
web_transfer_digest=$(sha256sum "$web_transfer_evidence" | cut -d' ' -f1)

# The run-time closure: Nix store paths the binaries reference.
runtime_refs=()
while IFS= read -r ref; do
  [ -e "$ref" ] && runtime_refs+=("$ref")
done < <(bash "$here/runtime-refs.sh" "$aura_bin" "$repl_bin")

remote_bin="${AURA_NIX_REMOTE_BIN:-/nix/var/nix/profiles/default/bin}"
rm -f "$links"/runtime-*
i=0
for ref in "${runtime_refs[@]}"; do
  "$(dirname "$nix_bin")/nix-store" --add-root "$links/runtime-$i" -r "$ref" >/dev/null
  i=$((i + 1))
done
if [ "${#runtime_refs[@]}" -gt 0 ]; then
  remote_program="${AURA_NIX_REMOTE_PROGRAM:-/nix/var/nix/profiles/default/bin/nix-daemon}"
  "$nix_bin" copy --no-check-sigs --to "ssh-ng://$AURA_E2E_REMOTE?remote-program=$remote_program" \
    "${runtime_refs[@]}"
fi

printf -v remote_checkout_cmd '%q ' bash -c '
set -euo pipefail
cd "$1"
store_bin=$2
expected=$3
public=$4
shift 4
git fetch -q origin
status=$(git status --porcelain --untracked-files=no)
[[ -z "$status" ]] || { printf "%s\n" "remote checkout has tracked changes" >&2; exit 1; }
git checkout -q main
git merge -q --ff-only "$expected"
[[ $(git rev-parse HEAD) == "$expected" ]] || { printf "%s\n" "remote checkout does not match requested commit" >&2; exit 1; }
mkdir -p bin target/release "$public" crates/aura-web/public/assets .nix-ship
find .nix-ship -maxdepth 1 -name "runtime-*" -delete
index=0
for ref in "$@"; do
    "$store_bin/nix-store" --add-root ".nix-ship/runtime-$index" -r "$ref" >/dev/null
    index=$((index + 1))
done
' bash "$remote_root" "$remote_bin" "$commit" "$web_public" "${runtime_refs[@]}"
ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "$remote_checkout_cmd"
ship_remote_rsync -a "$aura_bin" "$(ship_remote_destination "$remote_root/bin/aura")"
ship_remote_rsync -a "$repl_bin" "$(ship_remote_destination "$remote_root/target/release/tool_repl")"
ship_remote_rsync -a "$tailwind" "$(ship_remote_destination "$remote_root/$tailwind")"
RSYNC_OLD_ARGS=1 RSYNC_PROTECT_ARGS=0 bash "$here/ship-web-assets.sh" "$AURA_E2E_ROOT" "$web_public" \
  "$(ship_remote_destination "$remote_root/$web_public/")"

# The expected digest is selected by the validated producer and delivered in
# the authenticated command, independently of the protected input stream.
printf -v remote_provenance_cmd '%q ' bash -c \
  'cd "$1" && root=$(pwd -P) && exec "$2/nix" develop "$root" --command bash "$root/scripts/web/prebuilt-transfer.sh" install "$root" "$root/$3" "$4"' \
  bash "$remote_root" "$remote_bin" "$web_public" "$web_transfer_digest"
ssh -o BatchMode=yes -o StrictHostKeyChecking=yes "$AURA_E2E_REMOTE" \
  "$remote_provenance_cmd" < "$web_transfer_evidence"

printf -v remote_verify_cmd '%q ' bash -c \
  'cd "$1" && test "$(git rev-parse HEAD)" = "$2"' bash "$remote_root" "$commit"
ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "$remote_verify_cmd"

# Run the same binaries locally as on the remote host.
mkdir -p bin target/release
install -m 0755 "$aura_bin" bin/aura
install -m 0755 "$repl_bin" target/release/tool_repl
echo "ship: $commit shipped to $AURA_E2E_REMOTE (${#runtime_refs[@]} runtime store paths)"
