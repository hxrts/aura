#!/usr/bin/env bash
set -euo pipefail
prepared=$(cd "$(dirname "$0")" && pwd -P)
repo_root=$(git -C "$prepared" rev-parse --show-toplevel)
fixture_parent=$(cd "${TMPDIR:-/tmp}" && pwd -P)
fixture=$(mktemp -d "$fixture_parent/aura-ship-checkout.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
origin="$fixture/origin"
mkdir -p "$origin" "$fixture/remote bin"
git -C "$origin" init -qb main
printf 'first\n' > "$origin/source"
git -C "$origin" add .
export GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid
git -C "$origin" commit -qm first
commit=$(git -C "$origin" rev-parse HEAD)
positive="$fixture/remote checkout's"
negative="$fixture/ahead checkout's"
git clone -q "$origin" "$positive"
git clone -q "$origin" "$negative"
printf 'second\n' > "$origin/source"
git -C "$origin" commit -qam second
git -C "$negative" pull -q --ff-only
export ROOT_CALLS="$fixture/root-calls"
cat > "$fixture/remote bin/nix-store" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$ROOT_CALLS"
MOCK
chmod +x "$fixture/remote bin/nix-store"
ssh() {
  [[ $# == 4 && "$1" == -o && "$2" == BatchMode=yes && "$3" == fixture-host ]] || return 91
  if [[ "${FIXTURE_FAIL_STATUS:-0}" == 1 ]]; then
    PATH="$fixture/fail-bin:$PATH" zsh -f -c "$4"
  else
    zsh -f -c "$4"
  fi
}
AURA_E2E_REMOTE=fixture-host
remote_bin="$fixture/remote bin"
web_public=target/dx/aura-web/release/web/public
runtime_refs=(/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-fixture)
awk '/^printf -v remote_checkout_cmd / {copy=1} copy {print} copy && /^ssh .*"\$remote_checkout_cmd"$/ {exit}' \
  "$repo_root/scripts/harness/lan/ship.sh" > "$fixture/checkout-block.sh"
[[ -s "$fixture/checkout-block.sh" ]]
remote_root="$positive"
mkdir -p "$fixture/fail-bin"
real_git=$(command -v git)
cat > "$fixture/fail-bin/git" <<'MOCK'
#!/usr/bin/env bash
if [[ "${1:-}" == status ]]; then exit 47; fi
exec "$FIXTURE_REAL_GIT" "$@"
MOCK
chmod +x "$fixture/fail-bin/git"
export FIXTURE_REAL_GIT="$real_git" FIXTURE_FAIL_STATUS=1
if source "$fixture/checkout-block.sh" > "$fixture/status-failure.log" 2>&1; then
  echo 'failed remote Git status admitted checkout' >&2; exit 1
fi
[[ ! -d "$positive/target" && ! -d "$positive/.nix-ship" ]]
unset FIXTURE_FAIL_STATUS
printf 'dirty\n' > "$positive/source"
if source "$fixture/checkout-block.sh" > "$fixture/dirty-failure.log" 2>&1; then
  echo 'dirty remote checkout admitted' >&2; exit 1
fi
rg -q -- 'remote checkout has tracked changes' "$fixture/dirty-failure.log"
[[ ! -d "$positive/target" && ! -d "$positive/.nix-ship" && ! -e "$ROOT_CALLS" ]]
git -C "$positive" checkout -- source
mkdir -p "$positive/.nix-ship"
printf 'old-root\n' > "$positive/.nix-ship/runtime-old"
printf 'outer-glob\n' > "$positive/runtime-outer"
source "$fixture/checkout-block.sh" > "$fixture/positive.log" 2>&1
[[ ! -e "$positive/.nix-ship/runtime-old" && -f "$positive/runtime-outer" ]]
[[ -d "$positive/$web_public" && $(git -C "$positive" rev-parse HEAD) == "$commit" ]]
rg -q -- '--add-root .nix-ship/runtime-0 -r /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-fixture' "$ROOT_CALLS"
rm "$ROOT_CALLS"
mkdir -p "$negative/.nix-ship"
printf 'retained\n' > "$negative/.nix-ship/runtime-old"
remote_root="$negative"
if source "$fixture/checkout-block.sh" > "$fixture/negative.log" 2>&1; then
  echo 'ahead source accepted old candidate' >&2; exit 1
fi
rg -q -- 'remote checkout does not match requested commit' "$fixture/negative.log"
[[ ! -d "$negative/target" && ! -e "$ROOT_CALLS" && -f "$negative/.nix-ship/runtime-old" ]]
echo 'prepared ship exact-source/path quoting/refusal cases passed'
