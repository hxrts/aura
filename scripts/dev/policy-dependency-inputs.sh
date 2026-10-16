#!/usr/bin/env bash
# Input identities for the existing ownership and toolkit policy owners.
# Source this file; policy verdicts remain in the Rust checks.
policy_hash_files() {
  local root="$1" relative component current
  local -a components=()
  shift
  (( $# > 0 )) || return 2
  for relative in "$@"; do
    [[ "$relative" =~ ^[A-Za-z0-9_./-]+$ && "$relative" != /* &&
       "$relative" != ../* && "$relative" != */../* && "$relative" != */.. ]] || return 2
    current="$root"
    IFS=/ read -ra components <<< "$relative"
    for component in "${components[@]}"; do
      current="$current/$component"
      [[ ! -L "$current" ]] || return 2
    done
    [[ -f "$root/$relative" ]] || return 2
    printf '%s\0' "$relative"
    sha256sum "$root/$relative"
  done | sha256sum | awk '{print $1}'
}
policy_toolkit_inputs() {
  local root="$1" paths=() relative
  # This toolkit is an independent workspace with external crates locked by
  # its own Cargo.lock. A new build script or path dependency has additional
  # inputs this narrow graph cannot certify, so fail closed until declared.
  [[ ! -e "$root/toolkit/xtask/build.rs" && ! -L "$root/toolkit/xtask/build.rs" ]] || return 2
  if rg -q '(^|[,{[:space:]])(path|build)[[:space:]]*=' \
    "$root/toolkit/xtask/Cargo.toml"; then return 2; fi
  [[ -z "$(find "$root/toolkit/xtask/src" -type l -print -quit)" ]] || return 2
  for relative in toolkit/xtask/Cargo.toml toolkit/xtask/Cargo.lock flake.nix flake.lock; do
    paths+=("$relative")
  done
  while IFS= read -r relative; do paths+=("$relative"); done < <(
    cd "$root" && find toolkit/xtask/src -type f -print | LC_ALL=C sort
  )
  for relative in .cargo/config .cargo/config.toml toolkit/xtask/.cargo/config \
    toolkit/xtask/.cargo/config.toml rust-toolchain rust-toolchain.toml \
    toolkit/xtask/rust-toolchain toolkit/xtask/rust-toolchain.toml; do
    [[ ! -e "$root/$relative" && ! -L "$root/$relative" ]] || paths+=("$relative")
  done
  policy_hash_files "$root" "${paths[@]}"
}
policy_tool_identity() {
  local tool resolved key value selected cargo_home
  {
    for tool in rustc cargo; do
      resolved="$(command -v "$tool")"
      [[ -f "$resolved" ]] || return 2
      printf '%s\0' "$(cd "$(dirname "$resolved")" && pwd -P)/$(basename "$resolved")"
      sha256sum "$resolved"
      "$tool" -Vv
    done
    for key in RUSTC RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER; do
      selected="${!key-}"
      [[ -n "$selected" ]] || continue
      resolved="$(command -v "$selected")" || return 2
      [[ -f "$resolved" ]] || return 2
      printf '%s\0' "$(cd "$(dirname "$resolved")" && pwd -P)/$(basename "$resolved")"
      sha256sum "$resolved"
    done
    for key in RUSTC RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER CARGO_BUILD_TARGET CARGO_TARGET_DIR \
      CARGO_INCREMENTAL RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS \
      CARGO_PROFILE_DEV_OPT_LEVEL CARGO_PROFILE_DEV_DEBUG CARGO_PROFILE_DEV_INCREMENTAL \
      CARGO_HOME; do
      value="${!key-}"
      printf '%s\0%s\0' "$key" "$value"
    done
    cargo_home="${CARGO_HOME:-$HOME/.cargo}"
    for key in "$cargo_home/config" "$cargo_home/config.toml"; do
      [[ ! -e "$key" && ! -L "$key" ]] || {
        [[ -f "$key" && ! -L "$key" ]] || return 2
        sha256sum "$key"
      }
    done
  } | sha256sum | awk '{print $1}'
}
