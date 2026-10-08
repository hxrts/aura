#!/usr/bin/env bash
# Isolated checks for the LAN ship path (work/8.md Tasks 194 and 197); no
# build, ssh or Nix store change.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-lan-ship-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT
ship="$repo_root/scripts/harness/lan/ship.sh"

# runtime-refs: the store paths of the libraries the loader reports, once
# each, sorted; store paths only in the binary's strings are not included.
lib_a=/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-libiconv-109
lib_b=/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-openssl-3.0
toolchain=/nix/store/cccccccccccccccccccccccccccccccc-rust-default-1.93.0
fakebin="$test_root/fakebin"
mkdir -p "$fakebin"
cat > "$fakebin/otool" <<EOF
#!/usr/bin/env bash
printf '%s:\n' "\$2"
case "\$2" in
  *bin1) printf '\t%s/lib/libiconv.2.dylib (compat)\n\t/usr/lib/libSystem.B.dylib (compat)\n\t%s/lib/libssl.dylib (compat)\n' "$lib_a" "$lib_b" ;;
  *bin2) printf '\t%s/lib/libiconv.2.dylib (compat)\n' "$lib_a" ;;
esac
EOF
chmod +x "$fakebin/otool"
printf 'panic at %s/lib/rustlib/src/x.rs\0' "$toolchain" > "$test_root/bin1"
printf 'nothing here' > "$test_root/bin2"
refs="$(PATH="$fakebin:$PATH" bash "$repo_root/scripts/harness/lan/runtime-refs.sh" "$test_root/bin1" "$test_root/bin2")"
expected="$(printf '%s\n%s\n' "$lib_a" "$lib_b")"
[[ "$refs" == "$expected" ]] || { printf 'runtime-refs gave:\n%s\n' "$refs" >&2; exit 1; }
if bash "$repo_root/scripts/harness/lan/runtime-refs.sh" "$test_root/missing" 2>/dev/null; then
  echo 'runtime-refs accepted a missing binary' >&2; exit 1
fi

# nix-store-gc reports the ship roots it keeps.
cat > "$fakebin/nix-store" <<EOF
#!/usr/bin/env bash
[[ "\$*" == '--gc --print-roots' ]] || exit 0
printf '%s -> %s\n' /work/aura/.nix-ship/runtime-0 "$lib_a" /work/aura/.nix-ship/devshell /nix/store/cccccccccccccccccccccccccccccccc-shell /home/u/.nix-profile /nix/store/dddddddddddddddddddddddddddddddd-profile
EOF
cat > "$fakebin/nix" <<'EOF'
#!/usr/bin/env bash
[[ "$1 $2" == 'path-info --closure-size' ]] && printf '%s\t%s\n' "$3" 4096
exit 0
EOF
cat > "$fakebin/du" <<'EOF'
#!/usr/bin/env bash
printf '1\t/nix/store\n'
EOF
chmod +x "$fakebin"/*
report="$(PATH="$fakebin:$PATH" bash "$repo_root/scripts/dev/nix-store-gc.sh" --dry-run)"
grep -q "Keeps /work/aura/.nix-ship/runtime-0 -> $lib_a (closure 4096 bytes)" <<< "$report"
grep -q 'Keeps /work/aura/.nix-ship/devshell' <<< "$report"
grep -q 'Ship roots kept: 2' <<< "$report"

# ship.sh: clean-tree and commit checks kept; cargo `lan` builds through the
# budget; run-time closure rooted; no crate2nix workspace build.
grep -q 'checkout has uncommitted changes' "$ship"
grep -q 'checkout changed during the build' "$ship"
grep -q 'build-budget.sh --lane lan-ship' "$ship"
grep -q 'cargo build --profile lan -p aura-terminal --bin aura' "$ship"
grep -q 'cargo build --profile lan -p aura-harness --bin tool_repl' "$ship"
grep -q 'develop --profile "$links/devshell"' "$ship"
grep -q 'nix-store" --add-root "$links/runtime-' "$ship"
if grep -q 'aura-lan-terminal\|aura-lan-harness' "$ship" "$repo_root/justfile" "$repo_root/flake.nix"; then
  echo 'the crate2nix LAN ship build is still referenced' >&2; exit 1
fi
# Task 209: the local and remote harness stop before the first budgeted build
# (tool_repl and aura are build consumers), and the build waits its turn.
first_build="$(grep -n 'build-budget.sh --lane lan-ship' "$ship" | head -1 | cut -d: -f1)"
local_stop="$(grep -n 'bash "$here/drv.sh" stop' "$ship" | head -1 | cut -d: -f1)"
remote_stop="$(grep -n 'scripts/harness/lan/drv.sh stop' "$ship" | head -1 | cut -d: -f1)"
[[ -n "$local_stop" && -n "$remote_stop" && -n "$first_build" ]] || {
  echo 'ship.sh must stop the local and remote harness' >&2; exit 1;
}
(( local_stop < first_build && remote_stop < first_build )) || {
  echo 'ship.sh stops the harness after building' >&2; exit 1;
}
grep -q 'export AURA_BUILD_WAIT_SECONDS="${AURA_BUILD_WAIT_SECONDS:-[0-9]' "$ship"

# The lan profile keeps release optimization without whole-program LTO.
awk '/^\[profile.lan\]/{on=1; next} /^\[/{on=0} on' "$repo_root/Cargo.toml" > "$test_root/lan-profile"
grep -q '^inherits = "release"' "$test_root/lan-profile"
grep -q '^lto = false' "$test_root/lan-profile"

echo 'LAN ship checks passed'
