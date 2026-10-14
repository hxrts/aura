#!/usr/bin/env bash
# Rank only whole nonrelease cache lanes. Actual deletion always goes through
# prune-inactive-lane.sh, which rechecks locks, processes, open files and links.

aura_cache_rank_lanes() {
  local checkout="$1" priority="$2" preserved="$3" incremental="$4"
  local lane path size
  for lane in debug-incremental wasm-debug dylint trybuild; do
    [[ "$lane" != "$preserved" ]] || continue
    [[ "$lane" != debug-incremental || "$incremental" == 0 ]] || continue
    case "$lane" in
      debug-incremental) path="$checkout/target/debug/incremental" ;;
      wasm-debug) path="$checkout/target/wasm32-unknown-unknown/debug" ;;
      dylint) path="$checkout/target/dylint" ;;
      trybuild) path="$checkout/target/tests/trybuild" ;;
    esac
    [[ -d "$path" && ! -L "$path" ]] || continue
    size="$(du -sk "$path" 2>/dev/null | awk 'NR==1 {print $1}')" || continue
    [[ "$size" =~ ^[0-9]+$ ]] || continue
    printf '%s\t%012d\t%s\t%s\n' "$priority" "$size" "$checkout" "$lane"
  done
}

aura_cache_rank_candidates() {
  local checkout="$1" mode="$2" preserved="$3" incremental="$4"
  local candidate receipts recent field receipt terminal active
  local -a candidates=()
  if [[ "$mode" == headroom ]]; then
    while IFS= read -r -d '' field; do
      [[ "$field" == worktree\ * ]] || continue
      candidate="${field#worktree }"
      [[ "$candidate" != "$checkout" && "$candidate" != *$'\n'* && "$candidate" != *$'\t'* ]] || continue
      [[ -d "$candidate" && -f "$candidate/Cargo.toml" && ! -L "$candidate/target" ]] || continue
      receipts="$candidate/artifacts/disk-budget/receipts"
      [[ -d "$receipts" && ! -L "$receipts" ]] || continue
      recent="$(find "$receipts" -name receipt.json -type f -mmin -1440 -print -quit)"
      [[ -z "$recent" ]] || continue
      # An old receipt is useful evidence only after its child has terminated.
      terminal=0
      while IFS= read -r -d '' receipt; do
        if jq -e '.final_exit_status | type == "number"' "$receipt" >/dev/null 2>&1; then
          terminal=1
          break
        fi
      done < <(find "$receipts" -name receipt.json -type f -print0)
      [[ "$terminal" == 1 ]] || continue
      # A source edit or an editor/shell working in the checkout marks it active
      # even when no build has touched its receipt for a day.
      [[ -z "$(git -C "$candidate" status --porcelain --untracked-files=normal 2>/dev/null)" ]] || continue
      active="$(aura_scoped_processes "$candidate" 'bash|zsh|fish|code|Code|codex|claude|rust-analyzer|cargo|rustc|just|nvim|vim|emacs' "$$")"
      [[ -z "$active" ]] || continue
      candidates+=("$candidate")
    done < <(git -C "$checkout" worktree list --porcelain -z 2>/dev/null)
    for candidate in "${candidates[@]+"${candidates[@]}"}"; do
      aura_cache_rank_lanes "$candidate" 0 "$preserved" "$incremental"
    done
  fi
  aura_cache_rank_lanes "$checkout" 1 "$preserved" "$incremental"
}
