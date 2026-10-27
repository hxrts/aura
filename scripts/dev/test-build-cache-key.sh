#!/usr/bin/env bash
# Isolated key contract test; no vendor fetch or Cargo build.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
# These assertions inspect the native producer status explicitly, including
# when their caller is in an if/! /|| context that disables Bash errexit.
capture_key() {
  local status key bytes
  if "$@" > "$scratch/query.stdout" 2> "$scratch/query.stderr"; then status=0; else status=$?; fi
  cat "$scratch/query.stderr" >&2 || :
  [[ "$status" == 0 ]] || return "$status"
  key=$(cat "$scratch/query.stdout") || return "$?"
  bytes=$(wc -c < "$scratch/query.stdout") || return "$?"
  [[ "$bytes" -eq 64 && "$key" =~ ^[0-9a-f]{64}$ ]] || {
    echo 'build-cache-key fixture: successful producer did not return one raw 64-hex key' >&2
    return 78
  }
  printf '%s' "$key" || return "$?"
}
require_schema_refusal() {
  local cause=$1 status
  shift
  if "$@" > "$scratch/query.stdout" 2> "$scratch/query.stderr"; then status=0; else status=$?; fi
  cat "$scratch/query.stderr" >&2 || :
  if [[ "$status" != 1 ]]; then
    [[ "$status" != 0 ]] && return "$status"
    echo 'build-cache-key fixture: schema refusal unexpectedly returned success' >&2
    return 78
  fi
  [[ ! -s "$scratch/query.stdout" ]] || return 1
  # Nix may print a trace/header. Its one nonempty error cause must be the
  # exact throw from nix/build-cache-key.nix, not an EOF or evaluator failure.
  awk -v expected="$cause" '
    sub(/^[[:space:]]*error:[[:space:]]*/, "") && length {
      if ($0 == expected) found++; else unexpected=1
    }
    END { exit !(found == 1 && !unexpected) }
  ' "$scratch/query.stderr" || return 1
}
guard_contract() {
  local hash=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
  local native_error='original native query diagnostic'
  local cause='build-cache-key: missing required inputs'
  fake_query() { printf '%s' "$1"; printf '%s' "$2" >&2; return "$3"; }
  expect_status() {
    local expected=$1 actual
    shift
    if "$@" > "$scratch/control.stdout" 2> "$scratch/control.stderr"; then actual=0; else actual=$?; fi
    [[ "$actual" == "$expected" ]] || {
      printf 'key guard control: expected %s, got %s\n' "$expected" "$actual" >&2
      cat "$scratch/control.stderr" >&2 || :
      return 1
    }
  }
  expect_status 0 capture_key fake_query "$hash" '' 0 || return "$?"
  [[ $(cat "$scratch/control.stdout") == "$hash" ]] || return 1
  expect_status 139 capture_key fake_query "$hash" "$native_error" 139 || return "$?"
  [[ ! -s "$scratch/control.stdout" && $(cat "$scratch/query.stdout") == "$hash" ]] || return 1
  cmp "$scratch/query.stderr" "$scratch/control.stderr" || return "$?"
  expect_status 1 capture_key fake_query "$hash" "$native_error" 1 || return "$?"
  cmp "$scratch/query.stderr" "$scratch/control.stderr" || return "$?"
  expect_status 78 capture_key fake_query 'malformed' '' 0 || return "$?"
  expect_status 78 capture_key fake_query '' '' 0 || return "$?"
  expect_status 78 capture_key fake_query "$hash"$'\n' '' 0 || return "$?"
  expect_status 0 require_schema_refusal "$cause" fake_query '' "error: $cause" 1 || return "$?"
  expect_status 1 require_schema_refusal "$cause" fake_query '' 'error: unrelated native failure' 1 || return "$?"
  expect_status 139 require_schema_refusal "$cause" fake_query '' "error: $cause" 139 || return "$?"
  expect_status 78 require_schema_refusal "$cause" fake_query '' "error: $cause" 0 || return "$?"
  expect_status 1 require_schema_refusal "$cause" fake_query "$hash" "error: $cause" 1 || return "$?"
  expect_status 1 require_schema_refusal "$cause" fake_query '' '' 1 || return "$?"
  expect_status 1 require_schema_refusal "$cause" fake_query '' "error: $cause"$'\nerror: unrelated failure' 1 || return "$?"
  expect_status 1 require_schema_refusal "$cause" fake_query '' "error: $cause"$'\n'"error: $cause" 1 || return "$?"
  echo 'cache-key producer guards: 14 fake-only status/format/schema controls passed'
}
[[ $# == 0 || ( $# == 1 && $1 == --guard-contract ) ]] || exit 2
guard_contract || exit "$?"
[[ $# == 0 ]] || exit 0
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
base=$(capture_key query "$scratch/base.json") || exit "$?"
for field in platform devShellPath toolchainPath vendorPath lockHash \
  dummyManifestPath dummyConfigPath package profile target rustflags cc wrapper; do
  jq --arg field "$field" '.[$field] += "-changed"' "$scratch/base.json" > "$scratch/changed.json"
  changed=$(capture_key query "$scratch/changed.json") || exit "$?"
  [[ "$changed" != "$base" ]] || {
    echo "key omitted $field" >&2; exit 1;
  }
done
jq '.features += ["changed"]' "$scratch/base.json" > "$scratch/changed.json"
changed=$(capture_key query "$scratch/changed.json") || exit "$?"
[[ "$changed" != "$base" ]] || {
  echo 'key omitted features' >&2; exit 1;
}
jq '.incremental = true' "$scratch/base.json" > "$scratch/changed.json"
changed=$(capture_key query "$scratch/changed.json") || exit "$?"
[[ "$changed" != "$base" ]] || {
  echo 'key omitted incremental' >&2; exit 1;
}
jq '.buildEnv.CFLAGS = "-O2"' "$scratch/base.json" > "$scratch/changed.json"
changed=$(capture_key query "$scratch/changed.json") || exit "$?"
[[ "$changed" != "$base" ]] || {
  echo 'key omitted build-script environment' >&2; exit 1;
}
jq '.buildEnv.PRODUCER_DRV_PATH = "/nix/store/33333333333333333333333333333333-debug-producer.drv"' "$scratch/base.json" > "$scratch/changed.json"
changed=$(capture_key query "$scratch/changed.json") || exit "$?"
[[ "$changed" != "$base" ]] || {
  echo 'key omitted producer derivation' >&2; exit 1;
}
for expression in 'del(.vendorPath)' '.unexpected = "x"' '.features = "default"' '.buildEnv = []'; do
  jq "$expression" "$scratch/base.json" > "$scratch/changed.json"
  case "$expression" in
    'del(.vendorPath)') cause='build-cache-key: missing required inputs' ;;
    '.unexpected = "x"') cause='build-cache-key: unexpected inputs' ;;
    *) cause='build-cache-key: invalid input types' ;;
  esac
  require_schema_refusal "$cause" query "$scratch/changed.json" || exit "$?"
done
export AURA_BUILD_CACHE_KEY_INPUT="$scratch/base.json"
direct="$(cd "$repo_root" && capture_key nix eval --raw --impure --expr '
  (import ./nix/build-cache-key.nix)
    (builtins.fromJSON (builtins.readFile (builtins.getEnv "AURA_BUILD_CACHE_KEY_INPUT")))
')" || exit "$?"
[[ "$direct" == "$base" ]] || { echo 'shell and direct Nix keys differ' >&2; exit 1; }
flake="$(cd "$repo_root" && capture_key nix eval --raw --impure --expr '
  let
    outputs = builtins.getFlake ("git+file://" + toString ./.);
    input = builtins.fromJSON (builtins.readFile (builtins.getEnv "AURA_BUILD_CACHE_KEY_INPUT"));
  in outputs.lib.${builtins.currentSystem}.buildCacheKey input
')" || exit "$?"
[[ "$flake" == "$base" ]] || { echo 'flake and shell keys differ' >&2; exit 1; }
system="$(nix eval --raw --impure --expr builtins.currentSystem)" || exit "$?"
[[ -n "$system" ]] || exit 78
debug_direct="$(cd "$repo_root" && capture_key nix eval --raw ".#lib.${system}.debugCacheKey")" || exit "$?"
debug_shell="$(capture_key bash "$repo_root/scripts/dev/build-cache-key.sh" --debug)" || exit "$?"
[[ "$debug_shell" == "$debug_direct" ]] || { echo 'debug shell and flake keys differ' >&2; exit 1; }
echo 'build-cache-key: shared Nix/shell key, all dimensions and refusals passed'
