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

metadata_args=(metadata --manifest-path "$manifest_path" --format-version 1)
if [[ "${1:-}" == --selected-tool-inventory ]]; then
  # Admission is read-only: it cannot resolve a new lockfile or fetch inputs.
  metadata_args+=(--locked --offline)
fi
if ! package_metadata=$(cargo "${metadata_args[@]}"); then
  echo '[dx-runner] ERROR: failed to read package metadata' >&2
  exit 1
fi

resolve_package_version() {
  local package_name="$1"
  # A conflicting package-version selection is unsupported, not first-match.
  jq -er --arg pkg "$package_name" '
    [.packages[] | select(.name == $pkg) | .version] | unique |
    if length == 1 then .[0] else error("ambiguous or missing tool version") end
  ' <<< "$package_metadata"
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

cache_root="${AURA_WEB_TOOLS_CACHE_ROOT:-$HOME/.cache/aura/web-tools}"
dx_root="$cache_root/dioxus-cli/$dioxus_version"
wasm_root="$cache_root/wasm-bindgen-cli/$wasm_bindgen_version"
binaryen_system="$(nix eval --impure --raw --expr builtins.currentSystem)" || exit 1
expected_binaryen_root="$(nix eval --raw "git+file://$repo_root#packages.$binaryen_system.binaryen123.outPath")" || exit 1
binaryen_root="${AURA_NIX_BINARYEN_ROOT:-$expected_binaryen_root}"
if [[ ! "$binaryen_root" =~ ^/nix/store/[a-z0-9]{32}-binaryen-version_123$ ]]; then
  echo '[dx-runner] ERROR: Binaryen selection is not a version_123 Nix output' >&2
  exit 1
fi
if [[ "$binaryen_root" != "$expected_binaryen_root" ]]; then
  echo '[dx-runner] ERROR: foreign Binaryen output differs from pinned derivation' >&2
  exit 1
fi
if [[ -L "$binaryen_root" || ! -x "$binaryen_root/bin/wasm-opt" ||
      ! -d "$binaryen_root/lib" || -L "$binaryen_root/lib" ]]; then
  echo '[dx-runner] ERROR: pinned Nix Binaryen version_123 output unavailable' >&2
  exit 1
fi
dx_bin="$dx_root/bin/dx"
wasm_bin="$wasm_root/bin/wasm-bindgen"
wasm_opt_bin="$binaryen_root/bin/wasm-opt"
web_node_bin="$repo_root/crates/aura-web/node_modules/.bin"

if [[ "${1:-}" == --selected-tool-inventory ]]; then
  inventory="${2:-}"
  [[ -f "$inventory" && ! -L "$inventory" && -O "$inventory" &&
     "$(wc -c < "$inventory")" -eq 0 ]] || exit 2
  source "$script_dir/prebuilt-provenance.sh"
  for name in node npm cargo rustc; do
    aura_web_record_selected_tool "$inventory" "$name" "$(command -v "$name")"
  done
  aura_web_record_selected_tool "$inventory" dx "$dx_bin"
  aura_web_record_selected_tool "$inventory" wasm-bindgen "$wasm_bin"
  aura_web_record_selected_tool "$inventory" wasm-opt "$wasm_opt_bin"
  aura_web_record_binaryen_libraries "$inventory" "$binaryen_root/lib"
  exit 0
fi

if [[ -f Dioxus.toml ]]; then
  mkdir -p public/assets
fi

if [[ ! -x "$dx_bin" ]]; then
  install_cached_cargo_tool dioxus-cli "$dioxus_version" "$dx_root"
fi

if [[ ! -x "$wasm_bin" ]]; then
  install_cached_cargo_tool wasm-bindgen-cli "$wasm_bindgen_version" "$wasm_root"
fi

if [[ -d "$web_node_bin" ]]; then
  export PATH="$web_node_bin:$PATH"
fi

export PATH="$dx_root/bin:$wasm_root/bin:$binaryen_root/bin:$PATH"
export NO_DOWNLOADS=1

# Ensure cargo uses the workspace root target directory
export CARGO_TARGET_DIR="$repo_root/target"

if [[ -n "${AURA_WEB_TOOL_INVENTORY:-}" ]]; then
  source "$script_dir/prebuilt-provenance.sh"
  aura_web_record_selected_tool "$AURA_WEB_TOOL_INVENTORY" cargo "$(command -v cargo)"
  aura_web_record_selected_tool "$AURA_WEB_TOOL_INVENTORY" rustc "$(command -v rustc)"
  aura_web_record_selected_tool "$AURA_WEB_TOOL_INVENTORY" dx "$dx_bin"
  aura_web_record_selected_tool "$AURA_WEB_TOOL_INVENTORY" wasm-bindgen "$wasm_bin"
  aura_web_record_selected_tool "$AURA_WEB_TOOL_INVENTORY" wasm-opt "$wasm_opt_bin"
  aura_web_record_binaryen_libraries "$AURA_WEB_TOOL_INVENTORY" "$binaryen_root/lib"
fi

if [[ "${1:-}" != build ]]; then
  exec "$dx_bin" "$@"
fi

# dx defaults to --debug-symbols true, which keeps DWARF through wasm-bindgen
# and runs wasm-opt with --debuginfo instead of --strip-debug, so release
# bundles ship unstripped. Strip them unless a caller asks otherwise.
build_args=("$@")
if [[ " $* " == *" --release "* && " $* " != *" --debug-symbols"* ]]; then
  build_args+=(--debug-symbols false)
fi

# Builds leave a new content-hashed bundle behind each time; keep the newest.
status=0
"$dx_bin" "${build_args[@]}" || status=$?
if [[ "$status" -eq 0 ]]; then
  bash "$script_dir/prune-dx-assets.sh" "$CARGO_TARGET_DIR"
fi
exit "$status"
