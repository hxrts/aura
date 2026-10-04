#!/usr/bin/env bash
# Sequential fixed-source debug measurements, without another Git worktree.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
mode=dry
allow_live=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) mode=dry ;;
    --apply) mode=apply ;;
    --allow-live-harness) allow_live=1 ;;
    *) echo 'usage: compare-debug-incremental.sh [--dry-run|--apply] [--allow-live-harness]' >&2; exit 2 ;;
  esac
done
commit="$(git -C "$repo_root" rev-parse HEAD)"
printf 'Debug comparison: commit=%s; core check; incremental=1 then 0; clean, warm, source rebuild\n' "$commit"
[[ "$mode" == apply ]] || { echo 'Dry run: no build, source copy or deletion'; exit 0; }
[[ -n "${IN_NIX_SHELL:-}" ]] || { echo 'Run inside nix develop' >&2; exit 2; }
# Copy only tracked fixed-commit source. Existing checkout caches and E2E state
# are never copied or removed. Failure retains the snapshot for diagnosis.
mkdir -p "$repo_root/artifacts/disk-budget/debug-comparisons" "$repo_root/.tmp"
results="$(mktemp -d "$repo_root/artifacts/disk-budget/debug-comparisons/run.XXXXXX")"
snapshot="$(mktemp -d "$repo_root/.tmp/debug-comparison.XXXXXX")"
printf '%s\n' "$commit" > "$results/commit"
printf '%s\n' "$snapshot" > "$results/snapshot"
git -C "$repo_root" archive "$commit" | tar -x -C "$snapshot"
# Freeze the current guard as well: Bash reads scripts incrementally, so an
# edit to a running guard can corrupt its execution even after the build ends.
cp "$repo_root/scripts/dev/build-budget.sh" "$snapshot/scripts/dev/build-budget.sh"
# The lockfile is intentionally ignored in Aura; retain the current resolved
# dependency graph so both modes compile the same dependencies.
cp "$repo_root/Cargo.lock" "$snapshot/Cargo.lock"
printf 'mode\tstep\tseconds\ttarget_kib\tincremental_kib\n' > "$results/measurements.tsv"
budget_args=(--root "$snapshot" --no-prune)
(( allow_live == 0 )) || budget_args+=(--allow-live-harness)
for incremental in 1 0; do
  for step in clean warm rebuild; do
    if [[ "$step" == rebuild ]]; then
      # A harmless tracked-source change forces recompilation of the actual
      # foundation crate while retaining its dependency and incremental cache.
      printf '\n// Debug cache measurement: force source recompilation.\n' >> "$snapshot/crates/aura-core/src/lib.rs"
    fi
    started="$(date +%s)"
    if ! (cd "$snapshot"; unset CARGO_TARGET_DIR; export CARGO_INCREMENTAL="$incremental" CARGO_BUILD_JOBS=2;
      AURA_BUILD_PROFILE=debug AURA_BUILD_FEATURES=default \
      nice -n 10 bash "$snapshot/scripts/dev/build-budget.sh" "${budget_args[@]}" \
        --lane "debug-incremental-$incremental-$step" -- cargo check --locked -p hxrts-aura-core
    ) > "$results/$incremental-$step.log" 2>&1; then
      echo "Build failed; retained source=$snapshot evidence=$results" >&2
      exit 1
    fi
    elapsed=$(( $(date +%s) - started ))
    size="$(du -sk "$snapshot/target" | awk 'NR==1 {print $1}')"
    incremental_size=0
    if [[ -d "$snapshot/target/debug/incremental" ]]; then
      incremental_size="$(du -sk "$snapshot/target/debug/incremental" | awk 'NR==1 {print $1}')"
    fi
    printf '%s\t%s\t%s\t%s\t%s\n' "$incremental" "$step" "$elapsed" "$size" "$incremental_size" | tee -a "$results/measurements.tsv"
  done
  cp -R "$snapshot/artifacts/disk-budget" "$results/$incremental-budget"
  # Only this completed comparison's owned cache is removed, between modes.
  [[ ! -e "$snapshot/target/.aura-build-budget.lock" && ! -L "$snapshot/target" ]] || exit 1
  rm -rf -- "$snapshot/target"
  git -C "$repo_root" show "$commit:crates/aura-core/src/lib.rs" > "$snapshot/crates/aura-core/src/lib.rs"
done
rm -rf -- "$snapshot"
printf 'Evidence: %s\n' "$results"
