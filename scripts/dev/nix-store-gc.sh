#!/usr/bin/env bash
# Report or collect unreachable Nix store paths. Live GC roots (the ship
# roots under .nix-ship/: dev shell and shipped binaries' run-time paths,
# dev-shell profiles) are never collected.
#
# Usage: scripts/dev/nix-store-gc.sh [--dry-run|--apply] [--max SIZE]
#   --dry-run (default) prints the store size and the reclaimable dead paths.
#   --apply   runs `nix store gc`; --max bounds how much is freed (e.g. 20G).
set -euo pipefail

mode=--dry-run
max=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run|--apply) mode="$1" ;;
    --max) max="${2:?--max needs a size}"; shift ;;
    *) echo 'usage: nix-store-gc.sh [--dry-run|--apply] [--max SIZE]' >&2; exit 2 ;;
  esac
  shift
done

command -v nix >/dev/null 2>&1 || { echo 'nix-store-gc: nix not found' >&2; exit 1; }

store_kib="$(du -sk /nix/store 2>/dev/null | awk 'NR == 1 {print $1}' || true)"
printf 'Nix store: %s KiB\n' "${store_kib:-unavailable}"

# What GC keeps for the LAN runs: the ship roots (dev shell and the shipped
# binaries' run-time store paths) under any checkout's .nix-ship/.
kept=0
while IFS= read -r line; do
  link="${line%% -> *}"
  path="${line##* -> }"
  case "$link" in
    */.nix-ship/*)
      closure="$(nix path-info --closure-size "$path" 2>/dev/null | awk '{print $2}' || true)"
      printf 'Keeps %s -> %s (closure %s bytes)\n' "$link" "$path" "${closure:-unknown}"
      kept=$((kept + 1))
      ;;
  esac
done < <(nix-store --gc --print-roots 2>/dev/null || true)
printf 'Ship roots kept: %s\n' "$kept"

if [[ "$mode" == --dry-run ]]; then
  dead="$(nix-store --gc --print-dead 2>/dev/null || true)"
  count="$(printf '%s' "$dead" | grep -c . || true)"
  dead_kib=0
  if [[ "$count" -gt 0 ]]; then
    dead_kib="$(printf '%s\n' "$dead" | xargs du -sk 2>/dev/null | awk '{s += $1} END {print s + 0}')"
  fi
  printf 'Reclaimable: %s KiB in %s dead paths (dry run; pass --apply to collect)\n' "$dead_kib" "$count"
  exit 0
fi

args=(store gc)
[[ -z "$max" ]] || args+=(--max "$max")
nix "${args[@]}"
