#!/bin/bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd -P)
runner=$BASH
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-warm-rustc-wrapper.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
chmod 700 "$fixture"
cat > "$fixture/fake-cache" <<'CACHE'
#!/bin/bash
printf '%s\n' "$@" > "$AURA_TEST_ARGV"
exit "${AURA_TEST_EXIT:-0}"
CACHE
chmod 700 "$fixture/fake-cache"
sed "s|@sccache@|$fixture/fake-cache|g" "$repo/scripts/dev/warm-rustc-wrapper.sh.in" > "$fixture/rendered"
chmod 700 "$fixture/rendered"
bash -n "$fixture/rendered"
fail() { printf '%s\n' "warm-rustc-wrapper fixture: $1" >&2; exit 1; }
assert_line() {
  local actual
  actual=$(sed -n "${1}p" "$fixture/argv")
  [[ "$actual" == "$2" ]] || fail "argv line $1: expected '$2', got '$actual'"
}
assert_exit2() {
  local result=0
  "$@" >"$fixture/refusal.out" 2>&1 || result=$?
  [[ "$result" == 2 ]] || fail "expected refusal exit 2, got $result"
}

export AURA_TEST_ARGV="$fixture/argv"
export AURA_RUSTC_REMAP_FROM="$fixture/output"
"$runner" "$fixture/rendered" /pinned/rustc --version
assert_line 1 /pinned/rustc
assert_line 2 "--remap-path-prefix=$fixture/output=/aura-warm-output"
assert_line 3 --version
[[ $(wc -l < "$fixture/argv" | tr -d ' ') == 3 ]] || fail 'version probe gained arguments'
"$runner" "$fixture/rendered" /pinned/rustc -vV --print cfg 'a b'
assert_line 1 /pinned/rustc
assert_line 2 "--remap-path-prefix=$fixture/output=/aura-warm-output"
assert_line 3 -vV
assert_line 4 --print
assert_line 5 cfg
assert_line 6 'a b'
[[ $(wc -l < "$fixture/argv" | tr -d ' ') == 6 ]] || fail 'compiler probe gained arguments'
assert_exit2 env -u AURA_RUSTC_REMAP_FROM "$runner" "$fixture/rendered" /pinned/rustc --version
assert_exit2 env AURA_RUSTC_REMAP_FROM=relative "$runner" "$fixture/rendered" /pinned/rustc --version
assert_exit2 env AURA_RUSTC_REMAP_FROM=/ "$runner" "$fixture/rendered" /pinned/rustc --version
assert_exit2 "$runner" "$fixture/rendered"
if AURA_TEST_EXIT=37 "$runner" "$fixture/rendered" /pinned/rustc --version >"$fixture/exit.out" 2>&1; then
  fail 'compiler exit was not forwarded'
else
  [[ $? == 37 ]] || fail 'incorrect compiler exit'
fi
sed "s|@sccache@|$fixture/missing-cache|g" "$repo/scripts/dev/warm-rustc-wrapper.sh.in" > "$fixture/missing-rendered"
chmod 700 "$fixture/missing-rendered"
assert_exit2 "$runner" "$fixture/missing-rendered" /pinned/rustc --version
printf '%s\n' 'warm rustc wrapper safety tests passed'
