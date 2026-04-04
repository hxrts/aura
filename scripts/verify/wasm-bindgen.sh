#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
lockfile="$repo_root/Cargo.lock"

if [[ ! -f "$lockfile" ]]; then
  echo "[wasm-runner] ERROR: missing Cargo.lock at $lockfile" >&2
  exit 1
fi

wasm_bindgen_version="$(awk '
  $0 ~ /^name = "wasm-bindgen"$/ { in_pkg=1; next }
  in_pkg && $0 ~ /^version = / {
    gsub(/"/, "", $3);
    print $3;
    exit 0;
  }
  in_pkg && $0 ~ /^\[\[package\]\]$/ { in_pkg=0 }
' "$lockfile")"

if [[ -z "$wasm_bindgen_version" ]]; then
  echo "[wasm-runner] ERROR: failed to resolve wasm-bindgen version from Cargo.lock" >&2
  exit 1
fi

cache_root="${AURA_WASM_BINDGEN_CACHE_ROOT:-$HOME/.cache/aura/wasm-bindgen-cli}"
install_root="$cache_root/$wasm_bindgen_version"
runner="$install_root/bin/wasm-bindgen-test-runner"

if [[ ! -x "$runner" ]]; then
  echo "[wasm-runner] installing wasm-bindgen-cli $wasm_bindgen_version to $install_root" >&2
  cargo install --locked wasm-bindgen-cli --version "$wasm_bindgen_version" --root "$install_root" >/dev/null
fi

exec "$runner" "$@"
