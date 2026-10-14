#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-cache-rank-test.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
trap 'rm -rf "$fixture"' EXIT
checkout="$fixture/current"
git init -q "$checkout"
printf '[package]\nname="cache-rank-fixture"\nversion="0.1.0"\n' > "$checkout/Cargo.toml"
printf '/target/\n/artifacts/\n' > "$checkout/.gitignore"
git -C "$checkout" add Cargo.toml .gitignore
git -C "$checkout" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
git -C "$checkout" worktree add -q --detach "$fixture/stale" HEAD
git -C "$checkout" worktree add -q --detach "$fixture/recent" HEAD
git -C "$checkout" worktree add -q --detach "$fixture/unknown" HEAD
git -C "$checkout" worktree add -q --detach "$fixture/incomplete" HEAD
git -C "$checkout" worktree add -q --detach "$fixture/dirty" HEAD
git -C "$checkout" worktree add -q --detach "$fixture/active" HEAD
mkdir -p "$fixture/stale/artifacts/disk-budget/receipts/run.old" \
  "$fixture/recent/artifacts/disk-budget/receipts/run.recent" \
  "$fixture/incomplete/artifacts/disk-budget/receipts/run.old" \
  "$fixture/dirty/artifacts/disk-budget/receipts/run.old" \
  "$fixture/active/artifacts/disk-budget/receipts/run.old" \
  "$fixture/stale/target/dylint" "$fixture/stale/target/wasm32-unknown-unknown/wasm" \
  "$fixture/recent/target/dylint" "$fixture/unknown/target/dylint" \
  "$fixture/incomplete/target/dylint" "$fixture/dirty/target/dylint" \
  "$fixture/active/target/dylint" \
  "$checkout/target/wasm32-unknown-unknown/debug" "$checkout/target/tests/trybuild"
printf '{"final_exit_status":0}\n' > "$fixture/stale/artifacts/disk-budget/receipts/run.old/receipt.json"
printf '{}\n' > "$fixture/recent/artifacts/disk-budget/receipts/run.recent/receipt.json"
printf '{}\n' > "$fixture/incomplete/artifacts/disk-budget/receipts/run.old/receipt.json"
for owner in dirty active; do
  printf '{"final_exit_status":0}\n' > "$fixture/$owner/artifacts/disk-budget/receipts/run.old/receipt.json"
done
for owner in stale incomplete dirty active; do
  touch -t 202001010000 "$fixture/$owner/artifacts/disk-budget/receipts/run.old/receipt.json"
done
printf '# edited source\n' >> "$fixture/dirty/Cargo.toml"
dd if=/dev/zero of="$fixture/stale/target/dylint/cache" bs=1024 count=16 2>/dev/null
dd if=/dev/zero of="$checkout/target/wasm32-unknown-unknown/debug/cache" bs=1024 count=8 2>/dev/null
dd if=/dev/zero of="$checkout/target/tests/trybuild/cache" bs=1024 count=4 2>/dev/null
source "$repo_root/scripts/dev/cache-rank.sh"
source "$repo_root/scripts/dev/scoped-builders.sh"
# Simulate an editor holding a clean checkout without altering its sources.
aura_scoped_processes() {
  [[ "$1" == "$fixture/active" ]] && printf 'code(123) ' || true
}
ranked="$(aura_cache_rank_candidates "$checkout" headroom '' 0 | sort -t $'\t' -k1,1n -k2,2nr)"
[[ "$(printf '%s\n' "$ranked" | sed -n '1p')" == *$'\t'"$fixture/stale"$'\t'dylint ]]
[[ "$ranked" == *"$checkout"* ]]
for owner in recent unknown incomplete dirty active; do
  [[ "$ranked" != *"$fixture/$owner"* ]]
done
[[ "$ranked" != *wasm-profile* && "$ranked" != *"$fixture/stale"$'\t'wasm* ]]
current_only="$(aura_cache_rank_candidates "$checkout" soft-cap wasm-debug 1)"
[[ "$current_only" == *"$checkout"$'\t'trybuild* ]]
[[ "$current_only" != *"$fixture/stale"* && "$current_only" != *wasm-debug* &&
   "$current_only" != *debug-incremental* ]]
echo 'cache ranking: old terminal idle sibling first; recent/incomplete/dirty/active/protected omitted'
