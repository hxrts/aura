#!/usr/bin/env bash
# Preflight guards for the destructive clean-release benchmark.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-release-compare-test.XXXXXX")"
comparison_root="$repo_root/artifacts/disk-budget/comparisons"
mkdir -p "$comparison_root"
fixture="$(mktemp -d "$comparison_root/test-resume.XXXXXX")"
trap 'rm -rf "$test_root" "$fixture"' EXIT
fakebin="$test_root/fakebin"
mkdir -p "$fakebin"
export ACTIVE_FILE="$test_root/active"
export FREE_FILE="$test_root/free"
export PATH="$fakebin:$PATH"
cat > "$fakebin/ps" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$ACTIVE_FILE" ]] || printf '123 %s\n' "$(cat "$ACTIVE_FILE")"
EOF
cat > "$fakebin/df" <<'EOF'
#!/usr/bin/env bash
printf 'Filesystem 1024-blocks Used Available Capacity Mounted\n'
printf 'fake 99999999 0 %s 0%% /\n' "$(cat "$FREE_FILE")"
EOF
chmod +x "$fakebin"/*
compare() { bash "$repo_root/scripts/dev/compare-release-scopes.sh" "$@"; }
expect_status() {
  local expected="$1" actual
  shift
  if "$@" > "$test_root/output" 2>&1; then actual=0; else actual=$?; fi
  [[ "$actual" -eq "$expected" ]] || { cat "$test_root/output" >&2; exit 1; }
}
before="$(git -C "$repo_root" worktree list --porcelain | rg -c '^worktree ' )"
printf '%s\n' $((50 * 1024 * 1024)) > "$FREE_FILE"
expect_status 0 compare --dry-run
rg -q 'cargo-jobs=4' "$test_root/output"
expect_status 2 env AURA_COMPARE_CARGO_JOBS=zero bash "$repo_root/scripts/dev/compare-release-scopes.sh" --dry-run
printf 'tool_repl\n' > "$ACTIVE_FILE"
expect_status 1 compare --apply
rg -q 'consumer is active' "$test_root/output"
expect_status 1 compare --check
expect_status 0 compare --check --allow-live-harness
rg -q 'Preflight passed' "$test_root/output"
printf 'cargo\n' > "$ACTIVE_FILE"
expect_status 1 compare --check --allow-live-harness
rm "$ACTIVE_FILE"
printf '%s\n' $((10 * 1024 * 1024)) > "$FREE_FILE"
expect_status 1 compare --apply
rg -q 'at least 40 GiB' "$test_root/output"
printf '%s\n' $((50 * 1024 * 1024)) > "$FREE_FILE"
commit="$(git -C "$repo_root" rev-parse HEAD)"
for scope in terminal workspace; do
  printf 'After: free=50000000 KiB target=100 KiB exit=0\nCommit: %s\n' "$commit" > "$fixture/$scope.log"
  printf '4\n' > "$fixture/$scope-compiling-count"
  for kind in checkout target release; do printf '100\n' > "$fixture/$scope-$kind-kib"; done
done
expect_status 0 compare --apply --resume "$fixture"
rg -q 'Reusing completed terminal measurement' "$test_root/output"
rg -q 'Reusing completed workspace measurement' "$test_root/output"
rg -q 'Clean release: terminal=100 KiB workspace=100 KiB' "$test_root/output"
after="$(git -C "$repo_root" worktree list --porcelain | rg -c '^worktree ' )"
[[ "$before" -eq "$after" ]]
echo 'compare-release-scopes preflight tests passed'
