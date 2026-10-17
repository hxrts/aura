#!/usr/bin/env bash
# Input identities for the existing ownership and toolkit policy owners.
policy_exported_names() {
  # `compgen` is absent from the pinned noninteractive Nix Bash. Filter
  # values before Bash can trace them.
  export -p | awk '
    !/^declare -x [A-Za-z_][A-Za-z0-9_]*(=|$)/ { bad = 1; next }
    { sub(/^declare -x /, ""); sub(/=.*/, ""); print; count++ }
    END { if (bad || !count) exit 2 }
  '
}
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

# Identity of Git's effective input to the diff-aware static policy checks.
# Keep selector values out of receipts: only this digest is published.
policy_static_git_identity() {
  local root="$1" check="${2:-}" git_dir common_dir index_path key value config unsafe_config range base_ref
  for key in GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_OBJECT_DIRECTORY \
    GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_CONFIG GIT_CONFIG_COUNT GIT_CONFIG_GLOBAL \
    GIT_CONFIG_SYSTEM GIT_CONFIG_PARAMETERS GIT_CONFIG_NOSYSTEM GIT_COMMON_DIR \
    GIT_NAMESPACE GIT_SHALLOW_FILE GIT_REPLACE_REF_BASE GIT_EXTERNAL_DIFF \
    GIT_ATTR_NOSYSTEM GIT_ATTR_SOURCE; do
    [[ -z "${!key-}" ]] || return 2
  done
  [[ "$(git -C "$root" rev-parse --is-inside-work-tree)" == true ]] || return 2
  [[ "$(git -C "$root" rev-parse --show-toplevel)" == "$root" ]] || return 2
  git_dir="$(git -C "$root" rev-parse --absolute-git-dir)" || return 2
  common_dir="$(git -C "$root" rev-parse --path-format=absolute --git-common-dir)" || return 2
  policy_no_link_components "$git_dir" || return 2
  policy_no_link_components "$common_dir" || return 2
  [[ -d "$git_dir" && -d "$common_dir" ]] || return 2
  for config in "$git_dir/shallow" "$git_dir/info/grafts" \
    "$common_dir/shallow" "$common_dir/info/grafts" \
    "$git_dir/info/attributes" "$common_dir/info/attributes"; do
    [[ ! -e "$config" && ! -L "$config" ]] || return 2
  done
  [[ -z "$(git -C "$root" for-each-ref --format='%(refname)' refs/replace)" ]] || return 2
  # These can execute mutable external helpers or change diff input outside
  # the source/selector identity. The Rust check does not own that behavior.
  unsafe_config="$(git -C "$root" config --get-regexp \
    '^(diff\..*\.(textconv|command)|diff\.external|filter\..*\.(clean|process)|core\.(attributesfile|fsmonitor|sparsecheckout)|include\..*|includeif\..*)$' \
    2>/dev/null)" || { [[ $? == 1 ]] || return 2; }
  [[ -z "$unsafe_config" ]] || return 2
  # The Rust guidance owner historically treats a failed `git diff` as an
  # empty list. A fast preflight must refuse that failure before executing it.
  case "$check" in
    user-flow-policy-guardrails|user-flow-guidance-sync)
      range=''
      if [[ "$check" == user-flow-policy-guardrails ]]; then
        range="${AURA_UX_POLICY_DIFF_RANGE:-}"
      elif [[ -n "${AURA_UX_GUIDANCE_CHANGED_FILES+x}" ]]; then
        range=''
      else
        range="${AURA_UX_GUIDANCE_DIFF_RANGE:-}"
      fi
      if [[ -z "$range" ]] && ! {
        [[ "$check" == user-flow-guidance-sync && -n "${AURA_UX_GUIDANCE_CHANGED_FILES+x}" ]]
      }; then
        base_ref="${GITHUB_BASE_REF:-}"
        if [[ -n "$base_ref" ]] && git -C "$root" rev-parse --verify "origin/$base_ref" >/dev/null 2>&1; then
          range="origin/$base_ref...HEAD"
        else
          range=HEAD
        fi
      fi
      if [[ -n "$range" ]]; then
        [[ "$range" =~ ^[A-Za-z0-9_./^~-]+$ && "$range" != -* ]] || return 2
        local first="$range" second=''
        if [[ "$range" == *...* ]]; then
          first="${range%%...*}"
          second="${range#*...}"
        elif [[ "$range" == *..* ]]; then
          first="${range%%..*}"
          second="${range#*..}"
        fi
        [[ -n "$first" ]] || return 2
        git -C "$root" rev-parse --verify --end-of-options "${first}^{commit}" >/dev/null 2>&1 || return 2
        if [[ -n "$second" ]]; then
          git -C "$root" rev-parse --verify --end-of-options "${second}^{commit}" >/dev/null 2>&1 || return 2
        elif [[ "$first" != "$range" ]]; then
          return 2
        fi
        git -C "$root" diff --name-only "$range" -- >/dev/null || return 2
      fi
      ;;
    '') ;;
    *) return 2 ;;
  esac
  index_path="$(git -C "$root" rev-parse --git-path index)" || return 2
  [[ "$index_path" == /* ]] || index_path="$root/$index_path"
  policy_no_link_components "$index_path" || return 2
  [[ -f "$index_path" ]] || return 2
  {
    for key in AURA_UX_POLICY_DIFF_RANGE AURA_UX_GUIDANCE_DIFF_RANGE \
      AURA_UX_GUIDANCE_CHANGED_FILES GITHUB_BASE_REF; do
      value="${!key-}"
      printf '%s\0%s\0%s\0' "$key" "${!key+x}" "$value"
    done
    git -C "$root" rev-parse HEAD || return 2
    git -C "$root" for-each-ref --format='%(refname) %(objectname)' refs || return 2
    sha256sum "$index_path" || return 2
    # Effective values from system/global/local configuration are hashed,
    # never written to a receipt. Includes and external drivers are refused.
    git -C "$root" config --null --show-origin --list || return 2
    # A linked worktree has private config/index and a shared ref/config store.
    for config in "$common_dir/config" "$git_dir/config.worktree"; do
      [[ ! -e "$config" && ! -L "$config" ]] && continue
      policy_no_link_components "$config" || return 2
      [[ -f "$config" ]] || return 2
      printf '%s\0' "$config"
      sha256sum "$config" || return 2
    done
  } | sha256sum | awk '{print $1}'
}

policy_no_link_components() {
  local path="$1" current='' component
  local -a components=()
  [[ "$path" == /* ]] || return 2
  IFS=/ read -ra components <<< "$path"
  for component in "${components[@]}"; do
    [[ -n "$component" ]] || continue
    [[ "$component" != . && "$component" != .. ]] || return 2
    current="$current/$component"
    [[ ! -L "$current" ]] || return 2
  done
}

# The ownership lint binary is built from the normal/build dependency closure
# of aura-macros. A depfile only lists files read by one invocation and cannot
# certify new modules, manifests, or dependency edges. Hash whole local package
# directories and the selected registry closure instead.
policy_ownership_compile_inputs() {
  local root="$1" certification="${2:-reuse}" scratch metadata graph local_list registry_list
  local relative manifest package_dir archive checksum actual stage archive_hash source_hash tool_hash native_hash marker_hash
  local cargo_home="${CARGO_HOME:-$HOME/.cargo}" cargo_config
  [[ "$certification" == reuse || "$certification" == certify ]] || return 2
  for cargo_config in "$root/.cargo/config" "$root/.cargo/config.toml" \
    "$cargo_home/config" "$cargo_home/config.toml"; do
    [[ ! -e "$cargo_config" && ! -L "$cargo_config" ]] && continue
    [[ -f "$cargo_config" && ! -L "$cargo_config" ]] || return 2
    # Alternate sources make the crates.io archive/source relation ambiguous.
    if rg -q '^[[:space:]]*\[([[:space:]]*source\.|[[:space:]]*patch\.|[[:space:]]*replace\.)|^[[:space:]]*replace-with[[:space:]]*=' "$cargo_config"; then
      return 2
    fi
  done
  scratch="$(mktemp -d "${TMPDIR:-/tmp}/aura-ownership-inputs.XXXXXX")" || return 2
  chmod 700 "$scratch"
  metadata="$scratch/metadata.json"
  local_list="$scratch/local.tsv"
  registry_list="$scratch/registry.tsv"
  if ! (cd "$root" && cargo metadata --locked --offline --format-version 1 > "$metadata"); then
    rm -rf "$scratch"; return 2
  fi
  if ! jq -er '
    . as $m |
    ($m.resolve.nodes | map({key:.id,value:[.deps[] |
      select(any(.dep_kinds[]; .kind == null or .kind == "build")) | .pkg]}) | from_entries) as $edges |
    ($m.packages | map({key:.id,value:.}) | from_entries) as $packages |
    ($m.resolve.nodes | map({key:.id,value:(.features // [] | sort)}) | from_entries) as $features |
    ($m.packages[] | select(.name=="hxrts-aura-macros" and .source==null) | .id) as $start |
    [$start | recurse($edges[.][]?)] | unique | map($packages[.]) |
    if any(.[]; .source != null and .source != "registry+https://github.com/rust-lang/crates.io-index")
      then error("unsupported ownership dependency source") else . end |
    .[] | [.name,.version,(.source // "path"),.manifest_path,
      ([.targets[] | .kind | join(",")] | sort | join(";")),
      ($features[.id] | join(","))] | @tsv
  ' "$metadata" | LC_ALL=C sort > "$scratch/closure.tsv"; then
    rm -rf "$scratch"; return 2
  fi
  awk -F '\t' '$3=="path" {print}' "$scratch/closure.tsv" > "$local_list"
  awk -F '\t' '$3!="path" {print}' "$scratch/closure.tsv" > "$registry_list"
  if [[ "$(cut -f1 "$local_list" | LC_ALL=C sort | tr '\n' ' ')" != \
    'hxrts-aura-core hxrts-aura-macros hxrts-aura-mpst ' ]]; then
    rm -rf "$scratch"; return 2
  fi
  # No local build script or additional workspace path dependency is currently
  # certified. Fail closed if the manifest graph changes instead of borrowing
  # this receipt for a new compiler input owner.
  if jq -e '[.packages[] | select(.name=="hxrts-aura-core" or .name=="hxrts-aura-macros" or .name=="hxrts-aura-mpst") |
    any(.targets[]; any(.kind[]; .=="custom-build"))] | any' "$metadata" >/dev/null; then
    rm -rf "$scratch"; return 2
  fi
  local -a paths=(Cargo.toml Cargo.lock flake.nix flake.lock)
  local -a local_hashes=()
  policy_no_link_components "$root/crates" || { rm -rf "$scratch"; return 2; }
  while IFS=$'\t' read -r _ _ _ manifest _; do
    [[ "$manifest" == "$root/crates/"*/Cargo.toml ]] || { rm -rf "$scratch"; return 2; }
    package_dir="${manifest%/Cargo.toml}"
    if ! policy_no_link_components "$manifest"; then
      rm -rf "$scratch"; return 2;
    fi
    actual="$(policy_ownership_tree_hash "$package_dir")" || { rm -rf "$scratch"; return 2; }
    local_hashes+=("${package_dir#"$root"/}:$actual")
  done < "$local_list"
  for relative in .cargo/config .cargo/config.toml rust-toolchain rust-toolchain.toml; do
    [[ ! -e "$root/$relative" && ! -L "$root/$relative" ]] || paths+=("$relative")
  done
  source_hash="$(policy_hash_files "$root" "${paths[@]}")" || { rm -rf "$scratch"; return 2; }
  tool_hash="$(policy_tool_identity "$root" "$root")" || { rm -rf "$scratch"; return 2; }
  native_hash="$(policy_ownership_native_identity)" || { rm -rf "$scratch"; return 2; }
  {
    printf 'local:%s\n' "$source_hash"
    printf 'package:%s\n' "${local_hashes[@]}"
    printf 'closure:%s\n' "$(sha256sum "$scratch/closure.tsv" | awk '{print $1}')"
    while IFS=$'\t' read -r name version _ manifest _; do
      package_dir="${manifest%/Cargo.toml}"
      [[ "$manifest" == "$cargo_home/registry/src/"*/"$name-$version"/Cargo.toml ]] || {
        rm -rf "$scratch"; return 2;
      }
      policy_no_link_components "$manifest" || { rm -rf "$scratch"; return 2; }
      archive="$cargo_home/registry/cache/$(basename "$(dirname "$package_dir")")/$name-$version.crate"
      [[ -f "$archive" ]] && policy_no_link_components "$archive" || { rm -rf "$scratch"; return 2; }
      checksum="$(awk -v n="$name" -v v="$version" '
        /^\[\[package\]\]$/ {match_name=0; match_version=0; checksum=""}
        $0=="name = \"" n "\"" {match_name=1}
        $0=="version = \"" v "\"" {match_version=1}
        /^checksum = "/ && match_name && match_version {sub(/^checksum = "/, ""); sub(/"$/, ""); print; exit}
      ' "$root/Cargo.lock")"
      [[ "$checksum" =~ ^[a-f0-9]{64}$ ]] || { rm -rf "$scratch"; return 2; }
      archive_hash="$(sha256sum "$archive" | awk '{print $1}')"
      [[ "$archive_hash" == "$checksum" ]] || { rm -rf "$scratch"; return 2; }
      [[ -z "$(find "$package_dir" ! -type f ! -type d -print -quit)" ]] || { rm -rf "$scratch"; return 2; }
      [[ -f "$package_dir/.cargo-ok" && ! -L "$package_dir/.cargo-ok" ]] || {
        rm -rf "$scratch"; return 2;
      }
      marker_hash="$(sha256sum "$package_dir/.cargo-ok" | awk '{print $1}')"
      actual="$(policy_ownership_tree_hash "$package_dir" registry)" || { rm -rf "$scratch"; return 2; }
      if [[ "$certification" == certify ]]; then
        stage="$scratch/stage"
        mkdir -m 700 "$stage"
        # Archive bytes are locked. Still refuse traversal and special members
        # before extracting them under the private, one-package staging root.
        if ! tar -tf "$archive" > "$scratch/members" ||
          ! tar -tvf "$archive" > "$scratch/member-types" ||
          [[ ! -s "$scratch/members" ]]; then
          rm -rf "$scratch"; return 2
        fi
        cut -c1 "$scratch/member-types" > "$scratch/kinds" || { rm -rf "$scratch"; return 2; }
        [[ "$(wc -l < "$scratch/members")" == "$(wc -l < "$scratch/kinds")" ]] || {
          rm -rf "$scratch"; return 2;
        }
        if rg -q '[^d-]' "$scratch/kinds"; then rm -rf "$scratch"; return 2; fi
        while IFS= read -r relative; do
          [[ "$relative" == "$name-$version/"* ]] || { rm -rf "$scratch"; return 2; }
          relative="${relative#"$name-$version/"}"
          [[ -z "$relative" || ( "$relative" != /* &&
             "$relative" != .cargo-ok &&
             ! "$relative" =~ (^|/)\.\.(/|$) &&
             "$relative" =~ ^[A-Za-z0-9_.@/+(),\ -]+$ ) ]] || {
            rm -rf "$scratch"; return 2;
          }
        done < "$scratch/members"
        if ! tar -xf "$archive" -C "$stage"; then rm -rf "$scratch"; return 2; fi
        [[ -z "$(find "$stage" ! -type f ! -type d -print -quit)" ]] || { rm -rf "$scratch"; return 2; }
        [[ "$actual" == "$(policy_ownership_tree_hash "$stage/$name-$version" registry)" ]] || {
          rm -rf "$scratch"; return 2;
        }
        rm -rf "$stage"
      fi
      printf '%s:%s:%s:%s:%s\n' "$name" "$version" "$checksum" "$actual" "$marker_hash"
    done < "$registry_list"
    printf 'tools:%s\n' "$tool_hash"
    printf 'native:%s\n' "$native_hash"
  } | sha256sum | awk '{print $1}'
  local result="${PIPESTATUS[0]}"
  rm -rf "$scratch"
  [[ "$result" == 0 ]]
}

policy_ownership_native_identity() {
  local key value resolved tool sdkroot exported item
  local -a selectors=(
    CC CXX AR LD RANLIB HOST_CC TARGET_CC HOST_CXX TARGET_CXX HOST_AR TARGET_AR
    HOST_LD TARGET_LD CFLAGS CXXFLAGS CPPFLAGS LDFLAGS
    SDKROOT DEVELOPER_DIR MACOSX_DEPLOYMENT_TARGET
    PKG_CONFIG PKG_CONFIG_PATH PKG_CONFIG_LIBDIR PKG_CONFIG_SYSROOT_DIR
    BINDGEN_EXTRA_CLANG_ARGS LIBCLANG_PATH RUSTC_BOOTSTRAP
    HOST TARGET RUST_TARGET_PATH RUSTDOCFLAGS
    NIX_CC NIX_CFLAGS_COMPILE NIX_LDFLAGS NIX_CFLAGS_LINK NIX_DONT_SET_RPATH
  )
  exported="$(policy_exported_names)" || return 2
  while IFS= read -r key; do
    case "$key" in
      CFLAGS_*|CXXFLAGS_*|CPPFLAGS_*|LDFLAGS_*)
        [[ -z "${!key-}" ]] || return 2 ;;
    esac
  done <<< "$exported"
  for key in CFLAGS CXXFLAGS CPPFLAGS LDFLAGS BINDGEN_EXTRA_CLANG_ARGS; do
    [[ -z "${!key-}" ]] || return 2
  done
  for key in PKG_CONFIG_PATH PKG_CONFIG_LIBDIR PKG_CONFIG_SYSROOT_DIR LIBCLANG_PATH OPENSSL_DIR OPENSSL_LIB_DIR OPENSSL_INCLUDE_DIR; do
    value="${!key-}"
    [[ -z "$value" ]] && continue
    local -a locations=()
    IFS=: read -ra locations <<< "$value"
    for item in "${locations[@]}"; do
      [[ -d "$item" && "$item" == /nix/store/* ]] || return 2
      resolved="$(cd "$item" && pwd -P)" || return 2
      [[ "$resolved" == /nix/store/* ]] || return 2
    done
  done
  {
    # Bind Cargo's declared build-script selector families, including target
    # linker and native tool overrides, without recording raw values in the
    # receipt. Unknown executable override syntax refuses reuse.
    while IFS= read -r key; do
      case "$key" in
        CARGO_TARGET_*|CARGO_ENCODED_RUSTFLAGS|CARGO_BUILD_*|CARGO_PROFILE_*|CARGO_CFG_*|CARGO_FEATURE_*|\
        CC_*|CXX_*|AR_*|CFLAGS_*|CXXFLAGS_*|CPPFLAGS_*|LDFLAGS_*|\
        PKG_CONFIG_*|OPENSSL_*|LIBCLANG_*|BINDGEN_*|NIX_*) selectors+=("$key") ;;
      esac
    done < <(printf '%s\n' "$exported" | LC_ALL=C sort -u)
    while IFS= read -r key; do
      value="${!key-}"
      printf '%s\0%s\0' "$key" "$value"
      case "$key" in
        CC|CXX|AR|LD|RANLIB|PKG_CONFIG|HOST_CC|TARGET_CC|HOST_CXX|TARGET_CXX|HOST_AR|TARGET_AR|HOST_LD|TARGET_LD|CARGO_TARGET_*_LINKER|CC_*|CXX_*|AR_*)
          [[ "$key" != CC_ENABLE_DEBUG_OUTPUT ]] || continue
          [[ -z "$value" ]] && continue
          [[ "$value" != *[[:space:]]* ]] || return 2
          resolved="$(command -v "$value")" || return 2
          [[ -f "$resolved" ]] || return 2
          printf 'selected:%s:%s\0' "$key" "$(cd "$(dirname "$resolved")" && pwd -P)/$(basename "$resolved")"
          sha256sum "$resolved"
          ;;
      esac
    done < <(printf '%s\n' "${selectors[@]}" | LC_ALL=C sort -u)
    for tool in cc clang clang++ ar ld ranlib pkg-config; do
      resolved="$(command -v "$tool" 2>/dev/null || true)"
      [[ -z "$resolved" ]] && continue
      [[ -f "$resolved" ]] || return 2
      printf 'default:%s:%s\0' "$tool" "$(cd "$(dirname "$resolved")" && pwd -P)/$(basename "$resolved")"
      sha256sum "$resolved"
    done
    if [[ "$(uname -s)" == Darwin ]]; then
      for tool in clang clang++ ar ld; do
        resolved="$(xcrun --find "$tool" 2>/dev/null)" || return 2
        [[ -f "$resolved" ]] || return 2
        printf 'xcrun:%s:%s\0' "$tool" "$resolved"
        sha256sum "$resolved"
      done
      xcode-select -p
    fi
    sdkroot="${SDKROOT:-}"
    if [[ "$(uname -s)" == Darwin && -z "$sdkroot" ]]; then
      sdkroot="$(xcrun --show-sdk-path 2>/dev/null)" || return 2
    fi
    if [[ -n "$sdkroot" ]]; then
      [[ -d "$sdkroot" && "$sdkroot" == /nix/store/* ]] || return 2
      resolved="$(cd "$sdkroot" && pwd -P)" || return 2
      [[ "$resolved" == /nix/store/* ]] || return 2
      printf 'sdk:%s:%s\0' "$sdkroot" "$resolved"
    fi
  } | sha256sum | awk '{print $1}'
  local result="${PIPESTATUS[0]}"
  [[ "$result" == 0 ]]
}

policy_ownership_tree_hash() {
  local directory="$1" mode="${2:-local}"
  [[ -d "$directory" && ! -L "$directory" ]] || return 2
  [[ -z "$(find "$directory" ! -type f ! -type d -print -quit)" ]] || return 2
  (cd "$directory" && {
    find . -type d -print | LC_ALL=C sort | sed 's/^/dir:/'
    if [[ "$mode" == registry ]]; then
      find . -type f ! -path './.cargo-ok' -print0
    else
      find . -type f -print0
    fi | LC_ALL=C sort -z | xargs -0 shasum -a 256
  } | shasum -a 256 | awk '{print $1}')
}
policy_tool_identity() {
  local target_root="${1:?checkout root required}" cargo_cwd="${2:?Cargo working directory required}"
  local tool resolved key value selected cargo_home current config
  target_root="$(cd "$target_root" && pwd -P)" || return 2
  cargo_cwd="$(cd "$cargo_cwd" && pwd -P)" || return 2
  [[ "$cargo_cwd" == "$target_root" || "$cargo_cwd" == "$target_root/"* ]] || return 2
  policy_no_link_components "$target_root" || return 2
  policy_no_link_components "$cargo_cwd" || return 2
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
    # Cargo loads configuration from the invocation directory and every
    # ancestor, including parents outside the checkout. Bind addition and
    # removal as well as changed bytes; linked directories/files are unsafe.
    current="$cargo_cwd"
    while :; do
      [[ ! -L "$current/.cargo" &&
         ( ! -e "$current/.cargo" || -d "$current/.cargo" ) ]] || return 2
      for config in "$current/.cargo/config" "$current/.cargo/config.toml"; do
        [[ ! -e "$config" && ! -L "$config" ]] && continue
        [[ -f "$config" && ! -L "$config" ]] || return 2
        printf 'ancestor-config:%s\0' "$config"
        sha256sum "$config"
      done
      [[ "$current" != / ]] || break
      current="${current%/*}"
      [[ -n "$current" ]] || current=/
    done
  } | sha256sum | awk '{print $1}'
}
