#!/usr/bin/env bash
# Compare clean targeted and workspace release builds without touching the
# shared target or retained harness evidence. Requires an idle LAN host.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mode=dry
case "${1:-}" in
  ''|--dry-run) ;;
  --apply) mode=apply ;;
  *) echo 'usage: compare-release-scopes.sh [--dry-run|--apply]' >&2; exit 2 ;;
esac

commit="$(git -C "$repo_root" rev-parse HEAD)"
jobs="${AURA_COMPARE_CARGO_JOBS:-4}"
[[ "$jobs" =~ ^[0-9]+$ && "$jobs" -gt 0 ]] || {
  echo 'AURA_COMPARE_CARGO_JOBS must be a positive integer' >&2; exit 2;
}
free_kib() { df -Pk "$repo_root" | awk 'NR == 2 {print $4}'; }
consumers() {
  ps -axo pid=,comm= | awk '{n=$2;sub(/^.*\//,"",n);if(n~/^(cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep|tool_repl|aura-harness|aura)$/)printf "%s(%s) ",n,$1}'
}
printf 'Clean release comparison: commit=%s free=%s KiB mode=%s cargo-jobs=%s\n' \
  "$commit" "$(free_kib)" "$mode" "$jobs"
printf 'Order: clean terminal build, remove its owned worktree, clean workspace build, remove its owned worktree\n'
if [[ "$mode" == dry ]]; then
  printf 'Terminal: cargo build -p aura-terminal --bin aura --release --no-default-features --features terminal\n'
  printf 'Workspace: cargo build --workspace --release\n'
  exit 0
fi

[[ -z "$(consumers)" ]] || { echo "another build or harness consumer is active: $(consumers)" >&2; exit 1; }
(( $(free_kib) >= 40 * 1024 * 1024 )) || { echo 'need at least 40 GiB free for isolated clean-build comparison' >&2; exit 1; }
scratch="$(mktemp -d "${TMPDIR:-/tmp}/aura-release-compare.XXXXXX")"
scratch="$(cd "$scratch" && pwd -P)"
results="$repo_root/artifacts/disk-budget/comparisons/$(date -u +%Y%m%dT%H%M%SZ)-${commit:0:8}"
mkdir -p "$results"
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
  if (
    unset CARGO_TARGET_DIR
    cd "$repo_root"
    CARGO_BUILD_JOBS="$jobs" AURA_BUILD_TARGET_CAP_GIB=1000 \
      AURA_BUILD_PROFILE=release AURA_BUILD_FEATURES="$label" \
      nice -n 10 nix develop -c bash \
      "$repo_root/scripts/dev/build-budget.sh" --root "$worktree" \
      --lane "clean-$label" -- "$@"
  ) > "$log" 2>&1; then status=0; else status=$?; fi
  if [[ -d "$worktree/artifacts/disk-budget" ]]; then
    cp -R "$worktree/artifacts/disk-budget" "$results/$label-disk-budget"
  fi
  if (( status != 0 )); then
    echo "$label clean build failed (exit $status); inspect $log" >&2
    return "$status"
  fi
  printf '%s\n' "$(du -sk "$worktree/target" | awk '{print $1}')" > "$results/$label-target-kib"
  printf '%s\n' "$(du -sk "$worktree/target/release" | awk '{print $1}')" > "$results/$label-release-kib"
  compiling_count="$(rg -c '^   Compiling ' "$log" || true)"
  printf '%s\n' "${compiling_count:-0}" > "$results/$label-compiling-count"
  git -C "$repo_root" worktree remove --force "$worktree"
  current_worktree=''
  printf 'Finished %s; isolated target removed before next lane\n' "$label"
}

build_one terminal cargo build -p aura-terminal --bin aura --release --no-default-features --features terminal
build_one workspace cargo build --workspace --release
terminal_kib="$(cat "$results/terminal-target-kib")"
workspace_kib="$(cat "$results/workspace-target-kib")"
awk -v terminal="$terminal_kib" -v workspace="$workspace_kib" \
  'BEGIN {delta=workspace-terminal; percent=workspace>0?100*delta/workspace:0;
          printf "Clean target: terminal=%d KiB workspace=%d KiB saving=%d KiB (%.1f%%)\n", terminal, workspace, delta, percent}'
printf 'Evidence: %s\n' "$results"
