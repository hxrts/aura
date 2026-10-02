#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-prune-ci-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT
project="$test_root/project"
fakebin="$test_root/fakebin"
mkdir -p "$project/target" "$project/.tmp/e2e/run/failed" "$fakebin"
: > "$project/Cargo.toml"
printf 'failure evidence\n' > "$project/.tmp/e2e/run/failed/events.json"
printf 'outside\n' > "$test_root/outside"
ln -s "$test_root/outside" "$project/target/outside-link"
export CALLS_FILE="$test_root/calls"
export ACTIVE_FILE="$test_root/active"
export PROTECTED_FILE="$test_root/protected-release"
export OPEN_TARGET_FILE="$test_root/open-target"
export PATH="$fakebin:$PATH"
: > "$CALLS_FILE"

cat > "$fakebin/cargo" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$CALLS_FILE"
if [[ -f "$PROTECTED_FILE" && " $* " == *' --dry-run '* ]]; then
  printf '[DEBUG] Would remove: "%s/target/release/deps/libproduction.rlib"\n' "$PWD"
fi
EOF
cat > "$fakebin/ps" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$ACTIVE_FILE" ]] || printf '123 %s\n' "$(cat "$ACTIVE_FILE")"
EOF
cat > "$fakebin/lsof" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$OPEN_TARGET_FILE" ]] || cat "$OPEN_TARGET_FILE"
EOF
chmod +x "$fakebin"/*

expect_status() {
  local wanted="$1" actual
  shift
  if "$@" > "$test_root/output" 2>&1; then actual=0; else actual=$?; fi
  if [[ "$actual" -ne "$wanted" ]]; then
    cat "$test_root/output" >&2
    echo "expected $wanted, got $actual" >&2
    exit 1
  fi
}
run_prune() { bash "$repo_root/scripts/dev/prune-ci-cache.sh" --root "$project" "$@"; }

expect_status 0 run_prune --dry-run
[[ "$(wc -l < "$CALLS_FILE")" -eq 1 ]]
rg -q -- '--dry-run' "$CALLS_FILE"
[[ -f "$project/.tmp/e2e/run/failed/events.json" && -f "$test_root/outside" ]]
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

: > "$CALLS_FILE"
touch "$PROTECTED_FILE"
mkdir -p "$project/target/release" "$project/target/wasm32-unknown-unknown/debug"
printf 'keep\n' > "$project/target/release/production"
printf 'stale\n' > "$project/target/wasm32-unknown-unknown/debug/cache"
expect_status 0 run_prune --apply
if rg -v -- '--dry-run' "$CALLS_FILE" | rg -q .; then
  echo 'CI prune swept protected release cache' >&2
  exit 1
fi
[[ -f "$project/target/release/production" ]]
[[ ! -e "$project/target/wasm32-unknown-unknown/debug" ]]
[[ -f "$project/.tmp/e2e/run/failed/events.json" ]]
rm "$PROTECTED_FILE"

: > "$CALLS_FILE"
mkdir -p "$project/target/wasm32-unknown-unknown/debug"
printf 'stale\n' > "$project/target/wasm32-unknown-unknown/debug/cache"
printf 'fake 123 %s\n' "$project/target/release/production" > "$OPEN_TARGET_FILE"
expect_status 0 run_prune --apply
if rg -v -- '--dry-run' "$CALLS_FILE" | rg -q .; then
  echo 'CI prune swept a target containing an open file' >&2
  exit 1
fi
[[ -f "$project/target/release/production" ]]
[[ ! -e "$project/target/wasm32-unknown-unknown/debug" ]]
rm "$OPEN_TARGET_FILE"

: > "$CALLS_FILE"
expect_status 0 run_prune --apply
[[ "$(wc -l < "$CALLS_FILE")" -eq 2 ]]
[[ -f "$project/.tmp/e2e/run/failed/events.json" && -f "$test_root/outside" ]]
[[ -L "$project/target/outside-link" ]]
[[ ! -d "$project/target/.aura-build-budget.lock" ]]

: > "$CALLS_FILE"
printf 'tool_repl\n' > "$ACTIVE_FILE"
expect_status 1 run_prune --apply
[[ ! -s "$CALLS_FILE" ]]
rm -f "$ACTIVE_FILE"

mkdir "$project/target/.aura-build-budget.lock"
expect_status 1 run_prune --apply
rmdir "$project/target/.aura-build-budget.lock"

mv "$project/target" "$project/real-target"
ln -s "$project/real-target" "$project/target"
expect_status 1 run_prune --apply
[[ -f "$project/.tmp/e2e/run/failed/events.json" && -f "$test_root/outside" ]]

echo 'prune-ci-cache safety tests passed'
