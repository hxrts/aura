#!/usr/bin/env bash
# Query the same pure Nix key function exported from flake.nix. No Cargo
# dispatcher, vendor substitution, or target rehydration is enabled here.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [[ $# == 1 && "$1" == --debug ]]; then
  cd "$repo_root"
  system="$(nix eval --raw --impure --expr builtins.currentSystem)"
  exec nix eval --raw ".#lib.${system}.debugCacheKey"
fi
[[ $# == 1 && -f "$1" && ! -L "$1" ]] || {
  echo 'usage: build-cache-key.sh INPUT.json | --debug' >&2
  exit 2
}
input="$(cd "$(dirname "$1")" && pwd -P)/$(basename "$1")"
[[ "$(wc -c < "$input" | tr -d ' ')" -le 65536 ]] || {
  echo 'build-cache-key: input exceeds 64 KiB' >&2; exit 2;
}
jq -e 'type == "object"' "$input" >/dev/null || {
  echo 'build-cache-key: invalid JSON object' >&2; exit 2;
}
export AURA_BUILD_CACHE_KEY_INPUT="$input"
cd "$repo_root"
exec nix eval --raw --impure --expr '
  let
    input = builtins.fromJSON (builtins.readFile (builtins.getEnv "AURA_BUILD_CACHE_KEY_INPUT"));
  in (import ./nix/build-cache-key.nix) input
'
