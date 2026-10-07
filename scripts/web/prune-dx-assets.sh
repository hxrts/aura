#!/usr/bin/env bash
# Keep only the newest content-hashed aura-web wasm/js bundle in each dx
# assets directory. dx writes a new hashed bundle per build and never removes
# the old ones (about 15 MiB each).
set -euo pipefail

target_dir="${1:?usage: prune-dx-assets.sh <cargo-target-dir> [--dry-run]}"
mode="${2:-apply}"

shopt -s nullglob
for assets in "$target_dir"/dx/aura-web/*/web/public/assets; do
  [[ -d "$assets" && ! -L "$assets" ]] || continue
  for pattern in 'aura-web_bg-dxh*.wasm' 'aura-web-dxh*.js'; do
    # Newest first; everything after the first entry is stale.
    stale="$(cd "$assets" && ls -t -- $pattern 2>/dev/null | tail -n +2 || true)"
    [[ -n "$stale" ]] || continue
    while IFS= read -r name; do
      [[ "$name" == */* ]] && continue
      if [[ "$mode" == --dry-run ]]; then
        printf 'Would remove stale dx asset: %s/%s\n' "$assets" "$name"
      else
        rm -f -- "$assets/$name"
      fi
    done <<<"$stale"
  done
done
