#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-lane-prune-test.XXXXXX")"
test_root="$(cd "$test_root" && pwd -P)"
trap 'rm -rf "$test_root"' EXIT
project="$test_root/project"
fakebin="$test_root/fakebin"
mkdir -p "$project/target/debug" "$project/target/wasm32-unknown-unknown/debug" "$project/target/release" "$fakebin"
: > "$project/Cargo.toml"
: > "$project/target/debug/data"
: > "$project/target/wasm32-unknown-unknown/debug/data"
: > "$project/target/release/keep"
export ACTIVE_FILE="$test_root/active"
export OPEN_FILE="$test_root/open"
export PATH="$fakebin:$PATH"
cat > "$fakebin/ps" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$ACTIVE_FILE" ]] || printf '123 %s\n' "$(cat "$ACTIVE_FILE")"
EOF
cat > "$fakebin/lsof" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$OPEN_FILE" ]] || cat "$OPEN_FILE"
EOF
chmod +x "$fakebin"/*
prune() { bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$project" "$@"; }
expect_status() {
  local wanted="$1" actual
  shift
  if "$@" > "$test_root/output" 2>&1; then actual=0; else actual=$?; fi
  [[ "$actual" -eq "$wanted" ]] || { cat "$test_root/output" >&2; exit 1; }
}
expect_status 0 prune --lane wasm-debug --dry-run
[[ -f "$project/target/wasm32-unknown-unknown/debug/data" ]]

printf 'cargo\n' > "$ACTIVE_FILE"
expect_status 1 prune --lane wasm-debug --apply
rm "$ACTIVE_FILE"

printf '%s\n' "$project/target/wasm32-unknown-unknown/debug/data" > "$OPEN_FILE"
expect_status 1 prune --lane wasm-debug --apply
rm "$OPEN_FILE"

mkdir -p "$project/target/tests/trybuild"
: > "$project/target/tests/trybuild/cache"
expect_status 0 prune --lane trybuild --dry-run
[[ -f "$project/target/tests/trybuild/cache" ]]
printf 'cargo\n' > "$ACTIVE_FILE"
expect_status 1 prune --lane trybuild --apply
rm "$ACTIVE_FILE"
printf '%s\n' "$project/target/tests/trybuild/cache" > "$OPEN_FILE"
expect_status 1 prune --lane trybuild --apply
rm "$OPEN_FILE"
printf '%s\n' "$project/target/debug/data" > "$OPEN_FILE"
expect_status 0 prune --lane trybuild --apply
[[ ! -e "$project/target/tests/trybuild" && -f "$project/target/debug/data" ]]
rm "$OPEN_FILE"
rm -rf "$project/target/tests"
ln -s "$test_root" "$project/target/tests"
expect_status 1 prune --lane trybuild --dry-run
expect_status 1 prune --lane trybuild --apply
[[ -d "$test_root" && -f "$project/target/release/keep" ]]
rm "$project/target/tests"

expect_status 0 prune --lane wasm-debug --apply
[[ ! -e "$project/target/wasm32-unknown-unknown/debug" ]]
[[ -f "$project/target/debug/data" && -f "$project/target/release/keep" ]]

for lane in wasm-release wasm-host-release; do
  if [[ "$lane" == wasm-release ]]; then
    cache="$project/target/wasm32-unknown-unknown/wasm-release"
  else
    cache="$project/target/wasm-release"
  fi
  mkdir -p "$cache"
  : > "$cache/data"
  expect_status 0 prune --lane "$lane" --dry-run
  [[ -f "$cache/data" ]]
  for consumer in cargo tool_repl aura-harness; do
    printf '%s\n' "$consumer" > "$ACTIVE_FILE"
    expect_status 1 prune --lane "$lane" --apply
  done
  rm "$ACTIVE_FILE"
  printf '%s\n' "$cache/data" > "$OPEN_FILE"
  expect_status 1 prune --lane "$lane" --apply
  printf '%s\n' "$project/target/debug/data" > "$OPEN_FILE"
  expect_status 0 prune --lane "$lane" --apply
  [[ ! -e "$cache" && -f "$project/target/debug/data" && -f "$project/target/release/keep" ]]
  rm "$OPEN_FILE"
done

mkdir -p "$project/target/debug/incremental"
: > "$project/target/debug/incremental/cache"
expect_status 0 prune --lane debug-incremental --dry-run
[[ -f "$project/target/debug/incremental/cache" ]]
printf 'rustc\n' > "$ACTIVE_FILE"
expect_status 1 prune --lane debug-incremental --apply
rm "$ACTIVE_FILE"
printf '%s\n' "$project/target/debug/incremental/cache" > "$OPEN_FILE"
expect_status 1 prune --lane debug-incremental --apply
rm "$OPEN_FILE"
# An analyzer holding a sibling dependency must not block collection of the
# completed incremental lane or lose its loaded library.
printf '%s\n' "$project/target/debug/data" > "$OPEN_FILE"
expect_status 0 prune --lane debug-incremental --apply
[[ ! -e "$project/target/debug/incremental" && -f "$project/target/debug/data" ]]
rm "$OPEN_FILE"

rm -rf "$project/target/debug"
ln -s "$test_root" "$project/target/debug"
expect_status 1 prune --lane debug --apply
expect_status 1 prune --lane debug-incremental --dry-run
expect_status 1 prune --lane debug-incremental --apply
[[ -d "$test_root" && -f "$project/target/release/keep" ]]

expect_status 0 prune --lane release --dry-run
[[ -f "$project/target/release/keep" ]]
printf 'tool_repl\n' > "$ACTIVE_FILE"
expect_status 1 prune --lane release --apply
rm "$ACTIVE_FILE"
printf '%s\n' "$project/target/release/keep" > "$OPEN_FILE"
expect_status 1 prune --lane release --apply
rm "$OPEN_FILE"
expect_status 0 prune --lane release --apply
[[ ! -e "$project/target/release" && -L "$project/target/debug" && -d "$test_root" ]]
echo 'prune-inactive-lane safety tests passed'
