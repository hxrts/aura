#!/usr/bin/env bash
# Locate the workspace root and configure the Dioxus CLI environment.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script_repo_root="$(cd "$script_dir/../.." && pwd)"

find_workspace_root() {
  local candidate=""

  if [[ -n "${AURA_WORKSPACE_ROOT:-}" && -d "${AURA_WORKSPACE_ROOT}" ]]; then
    printf '%s\n' "${AURA_WORKSPACE_ROOT}"
    return 0
  fi

  if [[ -f "$script_repo_root/Cargo.toml" && -d "$script_repo_root/crates" ]]; then
    printf '%s\n' "$script_repo_root"
    return 0
  fi

  if candidate="$(git -C "$script_dir" rev-parse --show-toplevel 2>/dev/null)" && [[ -f "$candidate/Cargo.toml" && -d "$candidate/crates" ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi

  for candidate in "$script_dir" "$PWD"; do
    while [[ "$candidate" != "/" ]]; do
      if [[ -f "$candidate/Cargo.toml" && -d "$candidate/crates" ]]; then
        printf '%s\n' "$candidate"
        return 0
      fi
      candidate="$(dirname "$candidate")"
    done
  done

  return 1
}

repo_root="$(find_workspace_root || true)"
manifest_path="${repo_root:+$repo_root/crates/aura-web/Cargo.toml}"

if [[ -z "$repo_root" || ! -f "$manifest_path" ]]; then
  echo "[dx-runner] ERROR: failed to locate workspace root or aura-web manifest" >&2
  echo "[dx-runner] cwd=$PWD" >&2
  echo "[dx-runner] script_dir=$script_dir" >&2
  echo "[dx-runner] script_repo_root=$script_repo_root" >&2
  echo "[dx-runner] AURA_WORKSPACE_ROOT=${AURA_WORKSPACE_ROOT:-<unset>}" >&2
  echo "[dx-runner] manifest_path=${manifest_path:-<none>}" >&2
  echo "[dx-runner] AURA_WORKSPACE_ROOT/Cargo.toml=$(test -n "${AURA_WORKSPACE_ROOT:-}" && test -f "${AURA_WORKSPACE_ROOT}/Cargo.toml" && echo present || echo missing)" >&2
  echo "[dx-runner] script_repo_root/Cargo.toml=$(test -f "$script_repo_root/Cargo.toml" && echo present || echo missing)" >&2
  exit 1
fi

if [[ -t 1 && -z "${AURA_DX_LOG_REDIRECTED:-}" && "${AURA_DX_ALLOW_TTY:-0}" != "1" ]]; then
  mkdir -p "$repo_root/artifacts/aura-web"
  export AURA_DX_LOG_REDIRECTED=1
  export AURA_DX_LOG_FILE="${AURA_DX_LOG_FILE:-$repo_root/artifacts/aura-web/dx.log}"
  : >"$AURA_DX_LOG_FILE"
  exec >>"$AURA_DX_LOG_FILE" 2>&1
fi

resolve_package_version() {
  local package_name="$1"
  cargo metadata --manifest-path "$manifest_path" --format-version 1 2>/dev/null |
    jq -r --arg pkg "$package_name" '.packages[] | select(.name == $pkg) | .version' |
    head -n 1
}

dioxus_version="$(resolve_package_version dioxus)"
wasm_bindgen_version="$(resolve_package_version wasm-bindgen)"

if [[ -z "$dioxus_version" || -z "$wasm_bindgen_version" ]]; then
  echo "[dx-runner] ERROR: failed to resolve tool versions from cargo metadata" >&2
  echo "[dx-runner] manifest_path=$manifest_path" >&2
  echo "[dx-runner] dioxus_version=${dioxus_version:-<missing>}" >&2
  echo "[dx-runner] wasm_bindgen_version=${wasm_bindgen_version:-<missing>}" >&2
  exit 1
fi

install_cached_cargo_tool() {
  local package_name="$1"
  local package_version="$2"
  local install_root="$3"

  echo "[dx-runner] installing ${package_name} ${package_version} to ${install_root}" >&2
  if cargo install --locked --offline "$package_name" --version "$package_version" --root "$install_root" >/dev/null; then
    return 0
  fi

  echo "[dx-runner] offline install unavailable for ${package_name} ${package_version}; retrying with network" >&2
  rm -rf "$install_root"
  cargo install --locked "$package_name" --version "$package_version" --root "$install_root" >/dev/null
}

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
web_node_bin="$repo_root/crates/aura-web/node_modules/.bin"

if [[ -f Dioxus.toml ]]; then
  mkdir -p public/assets
fi

if [[ ! -x "$dx_bin" ]]; then
  install_cached_cargo_tool dioxus-cli "$dioxus_version" "$dx_root"
fi

if [[ ! -x "$wasm_bin" ]]; then
  install_cached_cargo_tool wasm-bindgen-cli "$wasm_bindgen_version" "$wasm_root"
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

if [[ -d "$web_node_bin" ]]; then
  export PATH="$web_node_bin:$PATH"
fi

export PATH="$dx_root/bin:$wasm_root/bin:$binaryen_root/bin:$PATH"
export NO_DOWNLOADS=1

# Ensure cargo uses the workspace root target directory
export CARGO_TARGET_DIR="$repo_root/target"

exec "$dx_bin" "$@"
