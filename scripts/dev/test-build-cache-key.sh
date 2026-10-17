#!/usr/bin/env bash
# Isolated key contract test; no vendor fetch or Cargo build.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
cat > "$scratch/base.json" <<'JSON'
{
  "platform": "aarch64-darwin",
  "devShellPath": "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-dev-shell",
  "toolchainPath": "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-rust",
  "vendorPath": "/nix/store/cccccccccccccccccccccccccccccccc-vendor",
  "lockHash": "sha256-original-lock",
  "dummyManifestPath": "/nix/store/dddddddddddddddddddddddddddddddd-dummy-manifest",
  "dummyConfigPath": "/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-dummy-config",
  "package": "hxrts-aura-core",
  "profile": "debug",
  "features": ["default"],
  "target": "aarch64-apple-darwin",
  "rustflags": "-C opt-level=0",
  "cc": "/nix/store/ffffffffffffffffffffffffffffffff-clang/bin/clang",
  "wrapper": "/nix/store/11111111111111111111111111111111-sccache/bin/sccache",
  "incremental": false,
  "buildEnv": { "CFLAGS": "-O0", "AR": "/nix/store/ar/bin/ar", "PRODUCER_DRV_PATH": "/nix/store/22222222222222222222222222222222-debug-producer.drv" }
}
JSON
query() { bash "$repo_root/scripts/dev/build-cache-key.sh" "$1"; }
base="$(query "$scratch/base.json")"
[[ "$base" =~ ^[0-9a-f]{64}$ ]] || { echo 'invalid key format' >&2; exit 1; }
for field in platform devShellPath toolchainPath vendorPath lockHash \
  dummyManifestPath dummyConfigPath package profile target rustflags cc wrapper; do
  jq --arg field "$field" '.[$field] += "-changed"' "$scratch/base.json" > "$scratch/changed.json"
  [[ "$(query "$scratch/changed.json")" != "$base" ]] || {
    echo "key omitted $field" >&2; exit 1;
  }
done
jq '.features += ["changed"]' "$scratch/base.json" > "$scratch/changed.json"
[[ "$(query "$scratch/changed.json")" != "$base" ]] || {
  echo 'key omitted features' >&2; exit 1;
}
jq '.incremental = true' "$scratch/base.json" > "$scratch/changed.json"
[[ "$(query "$scratch/changed.json")" != "$base" ]] || {
  echo 'key omitted incremental' >&2; exit 1;
}
jq '.buildEnv.CFLAGS = "-O2"' "$scratch/base.json" > "$scratch/changed.json"
[[ "$(query "$scratch/changed.json")" != "$base" ]] || {
  echo 'key omitted build-script environment' >&2; exit 1;
}
jq '.buildEnv.PRODUCER_DRV_PATH = "/nix/store/33333333333333333333333333333333-debug-producer.drv"' "$scratch/base.json" > "$scratch/changed.json"
[[ "$(query "$scratch/changed.json")" != "$base" ]] || {
  echo 'key omitted producer derivation' >&2; exit 1;
}
for expression in 'del(.vendorPath)' '.unexpected = "x"' '.features = "default"' '.buildEnv = []'; do
  jq "$expression" "$scratch/base.json" > "$scratch/changed.json"
  if query "$scratch/changed.json" > "$scratch/output" 2>&1; then
    echo "invalid key input accepted: $expression" >&2; exit 1;
  fi
done
export AURA_BUILD_CACHE_KEY_INPUT="$scratch/base.json"
direct="$(cd "$repo_root" && nix eval --raw --impure --expr '
  (import ./nix/build-cache-key.nix)
    (builtins.fromJSON (builtins.readFile (builtins.getEnv "AURA_BUILD_CACHE_KEY_INPUT")))
')"
[[ "$direct" == "$base" ]] || { echo 'shell and direct Nix keys differ' >&2; exit 1; }
flake="$(cd "$repo_root" && nix eval --raw --impure --expr '
  let
    outputs = builtins.getFlake ("git+file://" + toString ./.);
    input = builtins.fromJSON (builtins.readFile (builtins.getEnv "AURA_BUILD_CACHE_KEY_INPUT"));
  in outputs.lib.${builtins.currentSystem}.buildCacheKey input
')"
[[ "$flake" == "$base" ]] || { echo 'flake and shell keys differ' >&2; exit 1; }
system="$(nix eval --raw --impure --expr builtins.currentSystem)"
debug_direct="$(cd "$repo_root" && nix eval --raw ".#lib.${system}.debugCacheKey")"
debug_shell="$(bash "$repo_root/scripts/dev/build-cache-key.sh" --debug)"
[[ "$debug_shell" == "$debug_direct" ]] || { echo 'debug shell and flake keys differ' >&2; exit 1; }
echo 'build-cache-key: shared Nix/shell key, all dimensions and refusals passed'
