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
export PROJECT_ROOT="$project"
export OPEN_FILE="$test_root/open"
export PATH="$fakebin:$PATH"
cat > "$fakebin/ps" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$ACTIVE_FILE" ]] || printf '123 %s --out-dir %s/target/debug/deps\n' "$(cat "$ACTIVE_FILE")" "${ACTIVE_ROOT:-$PROJECT_ROOT}"
EOF
cat > "$fakebin/lsof" <<'EOF'
#!/usr/bin/env bash
[[ "${LSOF_EXIT:-0}" == 0 ]] || exit "$LSOF_EXIT"
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

for lane in wasm-profile wasm-release wasm-host-release; do
  if [[ "$lane" == wasm-profile ]]; then
    cache="$project/target/wasm32-unknown-unknown/wasm"
  elif [[ "$lane" == wasm-release ]]; then
    cache="$project/target/wasm32-unknown-unknown/wasm-release"
  else
    cache="$project/target/wasm-release"
  fi
  mkdir -p "$cache"
  : > "$cache/data"
  expect_status 0 prune --lane "$lane" --dry-run
  [[ -f "$cache/data" ]]
  for consumer in cargo rustc dx tool_repl aura-harness aura; do
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

# Exact actual Dioxus wasm profile: never remove neighboring profile outputs.
cache="$project/target/wasm32-unknown-unknown/wasm"
mkdir -p "$cache" "$project/target/wasm32-unknown-unknown/debug" "$project/target/wasm32-unknown-unknown/wasm-release"
: > "$cache/data"
: > "$project/target/wasm32-unknown-unknown/debug/keep"
: > "$project/target/wasm32-unknown-unknown/wasm-release/keep"
mkdir "$project/target/.aura-build-budget.lock"
expect_status 1 prune --lane wasm-profile --apply
[[ -f "$cache/data" ]]
rmdir "$project/target/.aura-build-budget.lock"
expect_status 1 prune --lane wasm-profile --lock-owned-by 0 --apply
[[ -f "$cache/data" ]]
# Missing inspection / inspection failure must not permit deletion.
missingbin="$test_root/missing-inspector"
mkdir "$missingbin"
for tool in bash dirname du awk mkdir rmdir cat ps rg; do
  ln -s "$(command -v "$tool")" "$missingbin/$tool"
done
PATH="$missingbin" expect_status 1 prune --lane wasm-profile --apply
rg -F 'lsof is required to prove this lane is idle' "$test_root/output" >/dev/null
[[ -f "$cache/data" ]]
LSOF_EXIT=44 expect_status 1 prune --lane wasm-profile --apply
[[ -f "$cache/data" ]]
mv "$cache" "$test_root/wasm-lane"
ln -s "$test_root/wasm-lane" "$cache"
expect_status 1 prune --lane wasm-profile --dry-run
expect_status 1 prune --lane wasm-profile --apply
[[ -f "$test_root/wasm-lane/data" ]]
rm "$cache"
mv "$test_root/wasm-lane" "$cache"
mv "$project/target/wasm32-unknown-unknown" "$test_root/wasm-parent"
ln -s "$test_root/wasm-parent" "$project/target/wasm32-unknown-unknown"
expect_status 1 prune --lane wasm-profile --apply
[[ -f "$test_root/wasm-parent/wasm/data" ]]
rm "$project/target/wasm32-unknown-unknown"
mv "$test_root/wasm-parent" "$project/target/wasm32-unknown-unknown"
mv "$project/target" "$test_root/target-parent"
ln -s "$test_root/target-parent" "$project/target"
expect_status 1 prune --lane wasm-profile --apply
[[ -f "$test_root/target-parent/wasm32-unknown-unknown/wasm/data" ]]
rm "$project/target"
mv "$test_root/target-parent" "$project/target"
# Foreign-checkout builder cannot block the exact idle lane.
printf 'cargo\n' > "$ACTIVE_FILE"
ACTIVE_ROOT="$test_root/sibling-worktree" expect_status 0 prune --lane wasm-profile --apply
rm "$ACTIVE_FILE"
[[ ! -e "$cache" ]]
[[ -f "$project/target/wasm32-unknown-unknown/debug/keep" && -f "$project/target/wasm32-unknown-unknown/wasm-release/keep" && -f "$project/target/debug/data" && -f "$project/target/release/keep" ]]
expect_status 0 prune --lane wasm-profile --apply
expect_status 2 prune --lane wasm --apply
printf 'actual wasm-profile isolation/refusal fixtures passed\n'

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

# Kani is an idle whole lane; a running kani driver blocks its removal.
mkdir -p "$project/target/kani"
: > "$project/target/kani/data"
printf 'kani-driver\n' > "$ACTIVE_FILE"
expect_status 1 prune --lane kani --apply
rm "$ACTIVE_FILE"
# A builder of a sibling worktree does not block this checkout's lane.
printf 'rustc\n' > "$ACTIVE_FILE"
ACTIVE_ROOT="$test_root/sibling-worktree" expect_status 0 prune --lane kani --apply
rm "$ACTIVE_FILE"
[[ ! -e "$project/target/kani" ]]
mkdir -p "$project/target/kani"
expect_status 0 prune --lane kani --apply
[[ ! -e "$project/target/kani" ]]

# The host-triple trybuild tree is removed without the default tree.
mkdir -p "$project/target/tests/trybuild/test-host-triple" "$project/target/tests/trybuild/debug"
: > "$project/target/tests/trybuild/debug/keep"
AURA_BUILD_TARGET_TRIPLE='bad/triple' expect_status 2 prune --lane trybuild-host-triple --dry-run
AURA_BUILD_TARGET_TRIPLE=test-host-triple expect_status 0 prune --lane trybuild-host-triple --apply
[[ ! -e "$project/target/tests/trybuild/test-host-triple" && -f "$project/target/tests/trybuild/debug/keep" ]]
echo 'prune-inactive-lane safety tests passed'
