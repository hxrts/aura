#!/usr/bin/env bash
# Preflight guards for the destructive clean-release benchmark.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-release-compare-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT
fakebin="$test_root/fakebin"
mkdir -p "$fakebin"
export ACTIVE_FILE="$test_root/active"
export FREE_FILE="$test_root/free"
export PATH="$fakebin:$PATH"
cat > "$fakebin/ps" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$ACTIVE_FILE" ]] || printf '123 tool_repl\n'
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
touch "$ACTIVE_FILE"
expect_status 1 compare --apply
rg -q 'consumer is active' "$test_root/output"
rm "$ACTIVE_FILE"
printf '%s\n' $((10 * 1024 * 1024)) > "$FREE_FILE"
expect_status 1 compare --apply
rg -q 'at least 40 GiB' "$test_root/output"
after="$(git -C "$repo_root" worktree list --porcelain | rg -c '^worktree ' )"
[[ "$before" -eq "$after" ]]
echo 'compare-release-scopes preflight tests passed'
