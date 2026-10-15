#!/usr/bin/env bash
# Copy the fixed build's public assets without retaining checkout-local links.
set -euo pipefail
[[ $# == 3 ]] || exit 2
repo_root=$(cd "$1" && pwd -P)
public=$2
destination=$3
[[ "$public" == target/dx/aura-web/release/web/public &&
   -d "$repo_root/$public" && ! -L "$repo_root/$public" ]] || exit 2
public_root="$repo_root/$public"
source_assets="$repo_root/crates/aura-web/public"
links=$(mktemp)
trap 'rm -f "$links"' EXIT
find "$public_root" -type l -print0 > "$links"

# Validate every link before rsync dereferences any of them. Build output may
# link to committed source assets or immutable Nix store files, never private
# run data or another checkout's files.
while IFS= read -r -d '' link; do
  resolved=$(realpath "$link") || {
    echo 'ship web assets: broken build-output link' >&2; exit 1;
  }
  [[ -f "$resolved" ]] || {
    echo 'ship web assets: build-output link is not a regular file' >&2; exit 1;
  }
  case "$resolved" in
    "$public_root"/*|"$source_assets"/*|/nix/store/*) ;;
    *) echo 'ship web assets: build-output link escapes trusted roots' >&2; exit 1 ;;
  esac
done < "$links"

rsync -a --copy-links --delete "$public_root/" "$destination"
