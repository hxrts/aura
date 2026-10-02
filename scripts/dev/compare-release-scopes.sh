#!/usr/bin/env bash
# Compare clean targeted and workspace release builds without touching the
# shared target or retained harness evidence. Requires an idle LAN host.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mode=dry
allow_live_harness=0
resume_dir=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) mode=dry ;;
    --check) mode=check ;;
    --apply) mode=apply ;;
    --allow-live-harness) allow_live_harness=1 ;;
    --resume) resume_dir="${2:?missing comparison directory}"; shift ;;
    *) echo 'usage: compare-release-scopes.sh [--dry-run|--check|--apply] [--allow-live-harness] [--resume RESULTS_DIR]' >&2; exit 2 ;;
  esac
  shift
done

commit="$(git -C "$repo_root" rev-parse HEAD)"
results=''
if [[ -n "$resume_dir" ]]; then
  [[ -d "$resume_dir" && ! -L "$resume_dir" ]] || {
    echo 'resume directory is missing or a symlink' >&2; exit 2;
  }
  results="$(cd "$resume_dir" && pwd -P)"
  comparison_root="$(cd "$repo_root/artifacts/disk-budget/comparisons" && pwd -P)"
  [[ "$results" == "$comparison_root/"* ]] || {
    echo 'resume directory is outside owned comparison results' >&2; exit 2;
  }
  if [[ -f "$results/commit" ]]; then
    commit="$(cat "$results/commit")"
  else
    commit="$(sed -n 's/^Commit: //p' "$results/terminal.log" | head -1)"
  fi
  [[ "$commit" =~ ^[0-9a-f]{40}$ ]] && git -C "$repo_root" cat-file -e "$commit^{commit}" || {
    echo 'resume directory lacks an available fixed commit' >&2; exit 2;
  }
fi
jobs="${AURA_COMPARE_CARGO_JOBS:-4}"
[[ "$jobs" =~ ^[0-9]+$ && "$jobs" -gt 0 ]] || {
  echo 'AURA_COMPARE_CARGO_JOBS must be a positive integer' >&2; exit 2;
}
free_kib() { df -Pk "$repo_root" | awk 'NR == 2 {print $4}'; }
consumers() {
  ps -axo pid=,comm= | awk -v allow="$allow_live_harness" \
    '{n=$2;sub(/^.*\//,"",n);builder=n~/^(cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep)$/;
      harness=n~/^(tool_repl|aura-harness|aura)$/;
      if(builder || (!allow && harness))printf "%s(%s) ",n,$1}'
}
printf 'Clean release comparison: commit=%s free=%s KiB mode=%s cargo-jobs=%s live-harness=%s\n' \
  "$commit" "$(free_kib)" "$mode" "$jobs" "$allow_live_harness"
printf 'Order: clean terminal build, remove its owned worktree, clean workspace build, remove its owned worktree\n'
[[ -z "$results" ]] || printf 'Resuming results: %s\n' "$results"
if [[ "$mode" == dry ]]; then
  printf 'Terminal: cargo build -p aura-terminal --bin aura --release --no-default-features --features terminal\n'
  printf 'Workspace: cargo build --workspace --release\n'
  exit 0
fi

[[ -z "$(consumers)" ]] || { echo "another build or harness consumer is active: $(consumers)" >&2; exit 1; }
(( $(free_kib) >= 40 * 1024 * 1024 )) || { echo 'need at least 40 GiB free for isolated clean-build comparison' >&2; exit 1; }
[[ "$mode" == check ]] && { echo 'Preflight passed: no build or deletion performed'; exit 0; }
scratch="$(mktemp -d "${TMPDIR:-/tmp}/aura-release-compare.XXXXXX")"
scratch="$(cd "$scratch" && pwd -P)"
if [[ -z "$results" ]]; then
  results="$repo_root/artifacts/disk-budget/comparisons/$(date -u +%Y%m%dT%H%M%SZ)-${commit:0:8}"
fi
mkdir -p "$results"
printf '%s\n' "$commit" > "$results/commit"
current_worktree=''
cleanup() {
  if [[ -n "$current_worktree" && -d "$current_worktree" ]]; then
    git -C "$repo_root" worktree remove --force "$current_worktree" 2>/dev/null || true
  fi
  rmdir "$scratch" 2>/dev/null || true
}
trap cleanup EXIT

build_one() {
  local label="$1" worktree="$scratch/$1" log="$results/$1.log" status compiling_count
  shift
  [[ -z "$(consumers)" ]] || { echo "another consumer started before $label" >&2; return 1; }
  current_worktree="$worktree"
  git -C "$repo_root" worktree add --detach "$worktree" "$commit" > "$results/$label-worktree.log" 2>&1
  printf 'Building %s from %s; log=%s\n' "$label" "$commit" "$log"
  local budget_args=(--root "$worktree" --lane "clean-$label" --no-prune)
  if (( allow_live_harness == 1 )); then budget_args+=(--allow-live-harness); fi
  if (
    unset CARGO_TARGET_DIR
    cd "$worktree"
    CARGO_BUILD_JOBS="$jobs" AURA_BUILD_TARGET_CAP_GIB=1000 \
      AURA_BUILD_PROFILE=release AURA_BUILD_FEATURES="$label" \
      nice -n 10 nix develop -c bash \
      "$worktree/scripts/dev/build-budget.sh" "${budget_args[@]}" -- "$@"
  ) > "$log" 2>&1; then status=0; else status=$?; fi
  if [[ -d "$worktree/artifacts/disk-budget" ]]; then
    cp -R "$worktree/artifacts/disk-budget" "$results/$label-disk-budget"
  fi
  if [[ -d "$worktree/target" ]]; then
    printf '%s\n' "$(du -sk "$worktree/target" | awk '{print $1}')" > "$results/$label-target-kib"
  fi
  if [[ -d "$worktree/target/release" ]]; then
    printf '%s\n' "$(du -sk "$worktree/target/release" | awk '{print $1}')" > "$results/$label-release-kib"
  fi
  printf '%s\n' "$(du -sk "$worktree" | awk '{print $1}')" > "$results/$label-checkout-kib"
  compiling_count="$(rg -c '^   Compiling ' "$log" || true)"
  printf '%s\n' "${compiling_count:-0}" > "$results/$label-compiling-count"
  if (( status != 0 )); then
    echo "$label clean build failed (exit $status); inspect $log" >&2
    return "$status"
  fi
  git -C "$repo_root" worktree remove --force "$worktree"
  current_worktree=''
  printf 'Finished %s; isolated target removed before next lane\n' "$label"
}

scope_ready() {
  local label="$1" scope
  [[ -f "$results/$label.log" ]] || return 1
  rg -q "^Commit: $commit$" "$results/$label.log" || return 1
  rg -q '^After: .* exit=0$' "$results/$label.log" || return 1
  for scope in checkout target release; do
    [[ -f "$results/$label-$scope-kib" ]] || return 1
    [[ "$(cat "$results/$label-$scope-kib")" =~ ^[0-9]+$ ]] || return 1
  done
  [[ -f "$results/$label-compiling-count" ]] || return 1
}
if scope_ready terminal; then
  echo 'Reusing completed terminal measurement from the fixed commit'
else
  build_one terminal cargo build -p aura-terminal --bin aura --release --no-default-features --features terminal
fi
if scope_ready workspace; then
  echo 'Reusing completed workspace measurement from the fixed commit'
else
  build_one workspace cargo build --workspace --release
fi
for scope in checkout target release; do
  terminal_kib="$(cat "$results/terminal-$scope-kib")"
  workspace_kib="$(cat "$results/workspace-$scope-kib")"
  awk -v scope="$scope" -v terminal="$terminal_kib" -v workspace="$workspace_kib" \
    'BEGIN {delta=workspace-terminal; percent=workspace>0?100*delta/workspace:0;
            printf "Clean %s: terminal=%d KiB workspace=%d KiB saving=%d KiB (%.1f%%)\n", scope, terminal, workspace, delta, percent}'
done
printf 'Compiled packages: terminal=%s workspace=%s\n' \
  "$(cat "$results/terminal-compiling-count")" \
  "$(cat "$results/workspace-compiling-count")"
printf 'Evidence: %s\n' "$results"
