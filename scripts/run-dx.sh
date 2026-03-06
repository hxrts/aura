#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
lockfile="$repo_root/Cargo.lock"

if [[ ! -f "$lockfile" ]]; then
  echo "[dx-runner] ERROR: missing Cargo.lock at $lockfile" >&2
  exit 1
fi

resolve_lock_version() {
  local package_name="$1"
  awk -v pkg="$package_name" '
    $0 ~ "^name = \"" pkg "\"$" { in_pkg=1; next }
    in_pkg && $0 ~ /^version = / {
      gsub(/"/, "", $3);
      print $3;
      exit 0;
    }
    in_pkg && $0 ~ /^\[\[package\]\]$/ { in_pkg=0 }
  ' "$lockfile"
}

dioxus_version="$(resolve_lock_version dioxus)"
wasm_bindgen_version="$(resolve_lock_version wasm-bindgen)"

if [[ -z "$dioxus_version" || -z "$wasm_bindgen_version" ]]; then
  echo "[dx-runner] ERROR: failed to resolve tool versions from Cargo.lock" >&2
  exit 1
fi

cache_root="${AURA_WEB_TOOLS_CACHE_ROOT:-$HOME/.cache/aura/web-tools}"
dx_root="$cache_root/dioxus-cli/$dioxus_version"
wasm_root="$cache_root/wasm-bindgen-cli/$wasm_bindgen_version"
dx_bin="$dx_root/bin/dx"
wasm_bin="$wasm_root/bin/wasm-bindgen"

if [[ -f Dioxus.toml && ! -d public ]]; then
  mkdir -p public
fi

if [[ ! -x "$dx_bin" ]]; then
  echo "[dx-runner] installing dioxus-cli $dioxus_version to $dx_root" >&2
  cargo install --locked dioxus-cli --version "$dioxus_version" --root "$dx_root" >/dev/null
fi

if [[ ! -x "$wasm_bin" ]]; then
  echo "[dx-runner] installing wasm-bindgen-cli $wasm_bindgen_version to $wasm_root" >&2
  cargo install --locked wasm-bindgen-cli --version "$wasm_bindgen_version" --root "$wasm_root" >/dev/null
fi

export PATH="$dx_root/bin:$wasm_root/bin:$PATH"

exec "$dx_bin" "$@"
