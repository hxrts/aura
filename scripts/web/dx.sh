#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
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

binaryen_version="version_123"

resolve_binaryen_archive() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os:$arch" in
    Linux:x86_64)
      echo "binaryen-${binaryen_version}-x86_64-linux.tar.gz"
      ;;
    Linux:aarch64|Linux:arm64)
      echo "binaryen-${binaryen_version}-aarch64-linux.tar.gz"
      ;;
    Darwin:x86_64)
      echo "binaryen-${binaryen_version}-x86_64-macos.tar.gz"
      ;;
    Darwin:arm64)
      echo "binaryen-${binaryen_version}-arm64-macos.tar.gz"
      ;;
    *)
      echo "[dx-runner] ERROR: unsupported platform for wasm-opt bootstrap: ${os}/${arch}" >&2
      exit 1
      ;;
  esac
}

cache_root="${AURA_WEB_TOOLS_CACHE_ROOT:-$HOME/.cache/aura/web-tools}"
dx_root="$cache_root/dioxus-cli/$dioxus_version"
wasm_root="$cache_root/wasm-bindgen-cli/$wasm_bindgen_version"
binaryen_root="$cache_root/binaryen/$binaryen_version"
dx_bin="$dx_root/bin/dx"
wasm_bin="$wasm_root/bin/wasm-bindgen"
wasm_opt_bin="$binaryen_root/bin/wasm-opt"

if [[ -f Dioxus.toml ]]; then
  mkdir -p public/assets
fi

if [[ ! -x "$dx_bin" ]]; then
  echo "[dx-runner] installing dioxus-cli $dioxus_version to $dx_root" >&2
  cargo install --locked dioxus-cli --version "$dioxus_version" --root "$dx_root" >/dev/null
fi

if [[ ! -x "$wasm_bin" ]]; then
  echo "[dx-runner] installing wasm-bindgen-cli $wasm_bindgen_version to $wasm_root" >&2
  cargo install --locked wasm-bindgen-cli --version "$wasm_bindgen_version" --root "$wasm_root" >/dev/null
fi

if [[ ! -x "$wasm_opt_bin" ]]; then
  archive_name="$(resolve_binaryen_archive)"
  archive_url="https://github.com/WebAssembly/binaryen/releases/download/${binaryen_version}/${archive_name}"
  temp_dir="$(mktemp -d)"
  trap 'rm -rf "$temp_dir"' EXIT
  echo "[dx-runner] installing wasm-opt ${binaryen_version} to $binaryen_root" >&2
  mkdir -p "$binaryen_root/bin" "$binaryen_root/lib"
  curl -L --fail --silent --show-error "$archive_url" -o "$temp_dir/binaryen.tar.gz"
  tar -xzf "$temp_dir/binaryen.tar.gz" -C "$temp_dir"
  archive_root="$(find "$temp_dir" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
  if [[ -z "$archive_root" || ! -x "$archive_root/bin/wasm-opt" ]]; then
    echo "[dx-runner] ERROR: failed to unpack wasm-opt from $archive_url" >&2
    exit 1
  fi
  cp "$archive_root/bin/wasm-opt" "$wasm_opt_bin"
  chmod +x "$wasm_opt_bin"
  if [[ -d "$archive_root/lib" ]]; then
    cp -R "$archive_root/lib/." "$binaryen_root/lib/"
  fi
fi

export PATH="$dx_root/bin:$wasm_root/bin:$binaryen_root/bin:$PATH"
export NO_DOWNLOADS=1

# Ensure cargo uses the workspace root target directory
export CARGO_TARGET_DIR="$repo_root/target"

exec "$dx_bin" "$@"
