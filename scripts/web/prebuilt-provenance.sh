#!/usr/bin/env bash
# Source-bound proof that the guarded Dioxus build completed on a clean commit.
# Dioxus may reuse byte-identical output without refreshing index.html mtime.

aura_web_clean_commit() {
    local repo_root=$1 status
    status=$(git -C "$repo_root" status --porcelain --untracked-files=normal) || return 1
    [[ -z "$status" ]] || return 1
    git -C "$repo_root" rev-parse --verify HEAD
}

aura_web_clean_tree() {
    local repo_root=$1
    aura_web_clean_commit "$repo_root" >/dev/null || return 1
    git -C "$repo_root" rev-parse --verify 'HEAD^{tree}'
}

aura_web_mode() {
    local path=$1 mode
    mode="$(stat -f %Lp "$path" 2>/dev/null || true)"
    [[ "$mode" =~ ^[0-7]{3,4}$ ]] || mode="$(stat -c %a "$path" 2>/dev/null)"
    printf '%s\n' "$mode"
}

aura_web_private_file() {
    local path=$1
    [[ -f "$path" && ! -L "$path" && -O "$path" &&
       "$(aura_web_mode "$path")" == 600 ]]
}

aura_web_build_config_digest() {
    local repo_root=$1 key config cargo_home exported
    # Guarded web builds set the repository's documented non-incremental
    # default. A read-only prebuilt consumer may not itself enter the guard.
    local effective_incremental="${CARGO_INCREMENTAL:-0}"
    local CARGO_INCREMENTAL="$effective_incremental"
    export CARGO_INCREMENTAL
    source "$repo_root/scripts/dev/policy-dependency-inputs.sh" || return 1
    cargo_home="${CARGO_HOME:-$HOME/.cargo}"
    exported="$(policy_exported_names)" || return 1
    {
        policy_tool_identity "$repo_root" "$repo_root/crates/aura-web" || return 1
        policy_ownership_native_identity || return 1
        for config in "$repo_root/.cargo/config" "$repo_root/.cargo/config.toml" \
            "$repo_root/crates/aura-web/.cargo/config" "$repo_root/crates/aura-web/.cargo/config.toml" \
            "$cargo_home/config" "$cargo_home/config.toml"; do
            [[ ! -e "$config" && ! -L "$config" ]] && continue
            [[ -f "$config" && ! -L "$config" ]] || return 1
            printf 'config:%s\0' "$config"
            sha256sum "$config" || return 1
        done
        while IFS= read -r key; do
            case "$key" in
                CARGO_HOME|CARGO_BUILD_TARGET|CARGO_TARGET_DIR|CARGO_INCREMENTAL|\
                CARGO_BUILD_JOBS|CARGO_BUILD_RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|\
                CARGO_PROFILE_*|CARGO_TARGET_*|RUSTFLAGS|RUSTC|RUSTC_WRAPPER|\
                RUSTC_WORKSPACE_WRAPPER|RUSTUP_TOOLCHAIN|RUSTDOCFLAGS|\
                CC|CXX|AR|LD|HOST_CC|TARGET_CC|HOST_AR|TARGET_AR|\
                AURA_WEB_TOOLS_CACHE_ROOT|AURA_NO_SCCACHE)
                    printf '%s\0%s\0' "$key" "${!key-}";;
            esac
        done < <(printf '%s\n' "$exported" | LC_ALL=C sort -u)
    } | sha256sum | cut -d' ' -f1
}

aura_web_bundle_digest() {
    local public_dir=$1
    [[ -d "$public_dir" && ! -L "$public_dir" &&
       -f "$public_dir/index.html" && ! -L "$public_dir/index.html" ]] || return 1
    local path=/ component
    local -a components
    IFS=/ read -ra components <<< "$public_dir"
    for component in "${components[@]}"; do
        [[ -n "$component" ]] || continue
        path="${path%/}/$component"
        [[ ! -L "$path" ]] || return 1
    done
    [[ -z "$(cd "$public_dir" && find . -type l ! -path './assets/tailwind.css' -print -quit)" ]] || return 1
    (
        cd "$public_dir"
        find . -type f \( -name 'index.html' -o -name '*.js' -o -name '*.wasm' \) -print0 \
            | sort -z | xargs -0 sha256sum | sha256sum | cut -d' ' -f1
    )
}

aura_web_toolchain_lock_digest() {
    local repo_root=$1
    [[ -f "$repo_root/flake.lock" && ! -L "$repo_root/flake.lock" ]] || return 1
    sha256sum "$repo_root/flake.lock" | cut -d' ' -f1
}

aura_web_record_selected_tool() {
    local inventory=$1 name=$2 selected=$3 digest
    aura_web_private_file "$inventory" || return 1
    [[ "$name" =~ ^(node|npm|cargo|rustc|dx|wasm-bindgen|wasm-opt)$ &&
       "$selected" == /* && "$selected" != *'|'* &&
       "$selected" != *$'\n'* &&
       -f "$selected" && -x "$selected" ]] || return 1
    digest=$(sha256sum "$selected" | cut -d' ' -f1) || return 1
    printf '%s|%s|%s\n' "$name" "$selected" "$digest" >> "$inventory"
}

aura_web_binaryen_library_digest() {
    local directory=$1
    [[ -d "$directory" && ! -L "$directory" &&
       -n "$(find "$directory" -type f -print -quit)" &&
       -z "$(find "$directory" ! -type f ! -type d -print -quit)" ]] || return 1
    (cd "$directory" && find . -type f -print0 | LC_ALL=C sort -z |
        xargs -0 sha256sum | sha256sum | cut -d' ' -f1)
}

aura_web_record_binaryen_libraries() {
    local inventory=$1 directory=$2 digest
    aura_web_private_file "$inventory" || return 1
    [[ "$directory" == /* && "$directory" != *'|'* && "$directory" != *$'\n'* ]] || return 1
    digest=$(aura_web_binaryen_library_digest "$directory") || return 1
    printf 'binaryen-lib-tree|%s|%s\n' "$directory" "$digest" >> "$inventory"
}

aura_web_selected_tools_match() {
    local inventory=$1 line name selected digest actual extra
    local -a expected
    aura_web_private_file "$inventory" || return 1
    expected=(node npm cargo rustc dx wasm-bindgen wasm-opt binaryen-lib-tree)
    [[ $(wc -l < "$inventory") -eq ${#expected[@]} ]] || return 1
    local index=0
    while IFS= read -r line; do
        IFS='|' read -r name selected digest extra <<< "$line"
        [[ "$name" == "${expected[$index]}" && -z "${extra:-}" &&
           "$selected" == /* && "$digest" =~ ^[0-9a-f]{64}$ ]] || return 1
        if [[ "$name" == binaryen-lib-tree ]]; then
            actual=$(aura_web_binaryen_library_digest "$selected") || return 1
        else
            [[ -f "$selected" && -x "$selected" ]] || return 1
            actual=$(sha256sum "$selected" | cut -d' ' -f1) || return 1
        fi
        [[ "$actual" == "$digest" ]] || return 1
        case "$name" in
            node|npm|cargo|rustc)
                [[ "$(command -v "$name")" == "$selected" ]] || return 1;;
        esac
        index=$((index + 1))
    done < "$inventory"
}

aura_web_build_args=(build --release --profile wasm --platform web --package aura-web --bin aura-web --features web,harness,production-build)
aura_web_build_command="dx ${aura_web_build_args[*]}"

aura_web_write_prebuilt_provenance() {
    local repo_root=$1 public_dir=$2 inventory=$3 start_commit=$4 start_tree=$5 start_config=$6 marker commit tree config_digest digest toolchain scratch inventory_hash private_dir protected_inventory private_scratch
    marker="$public_dir/.aura-prebuilt-provenance"
    [[ ! -L "$marker" ]] || return 1
    # Ordinary dirty development builds remain usable, but cannot claim a
    # fixed-source prebuilt bundle for a LAN harness run.
    commit=$(aura_web_clean_commit "$repo_root") || return 0
    tree=$(aura_web_clean_tree "$repo_root") || return 0
    config_digest=$(aura_web_build_config_digest "$repo_root") || return 1
    [[ -n "$start_commit" && "$commit" == "$start_commit" &&
       -n "$start_tree" && "$tree" == "$start_tree" &&
       -n "$start_config" && "$config_digest" == "$start_config" ]] || return 0
    digest=$(aura_web_bundle_digest "$public_dir") || return 1
    toolchain=$(aura_web_toolchain_lock_digest "$repo_root") || return 1
    aura_web_selected_tools_match "$inventory" || return 1
    inventory_hash=$(sha256sum "$inventory" | cut -d' ' -f1) || return 1
    [[ "$commit" =~ ^[0-9a-f]{40}$ && "$digest" =~ ^[0-9a-f]{64}$ &&
       "$toolchain" =~ ^[0-9a-f]{64}$ ]] || return 1
    private_dir="$repo_root/artifacts/aura-web/private-provenance"
    [[ ! -L "$repo_root/artifacts" && ! -L "$repo_root/artifacts/aura-web" &&
       ! -L "$private_dir" ]] || return 1
    mkdir -p "$repo_root/artifacts/aura-web"
    [[ -e "$private_dir" ]] || mkdir -m 700 "$private_dir"
    [[ -d "$private_dir" && -O "$private_dir" &&
       "$(aura_web_mode "$private_dir")" == 700 ]] || return 1
    protected_inventory="$private_dir/prebuilt-tools-$commit-$inventory_hash"
    [[ ! -e "$protected_inventory" && ! -L "$protected_inventory" ]] ||
        aura_web_private_file "$protected_inventory" || return 1
    private_scratch=$(mktemp "$private_dir/prebuilt-tools.XXXXXX") || return 1
    cat "$inventory" > "$private_scratch"
    chmod 600 "$private_scratch"
    mv -f "$private_scratch" "$protected_inventory"
    scratch=$(mktemp "$public_dir/.aura-prebuilt-provenance.XXXXXX") || return 1
    printf 'commit=%s\ntoolchain_lock_sha256=%s\nbuild_command=%s\nbundle_sha256=%s\nsource_tree=%s\ntool_inventory_sha256=%s\nbuild_config_sha256=%s\n' \
        "$commit" "$toolchain" "$aura_web_build_command" "$digest" "$tree" "$inventory_hash" "$config_digest" > "$scratch"
    chmod 600 "$scratch"
    mv -f "$scratch" "$marker"
}

aura_web_prebuilt_provenance_matches() {
    local repo_root=$1 public_dir=$2 marker commit tree config_digest digest toolchain command inventory_hash expected_commit expected_tree expected_config expected_digest expected_toolchain
    marker="$public_dir/.aura-prebuilt-provenance"
    aura_web_private_file "$marker" || return 1
    expected_commit=$(aura_web_clean_commit "$repo_root") || return 1
    expected_tree=$(aura_web_clean_tree "$repo_root") || return 1
    expected_config=$(aura_web_build_config_digest "$repo_root") || return 1
    expected_digest=$(aura_web_bundle_digest "$public_dir") || return 1
    expected_toolchain=$(aura_web_toolchain_lock_digest "$repo_root") || return 1
    commit=$(sed -n '1s/^commit=//p' "$marker")
    toolchain=$(sed -n '2s/^toolchain_lock_sha256=//p' "$marker")
    command=$(sed -n '3s/^build_command=//p' "$marker")
    digest=$(sed -n '4s/^bundle_sha256=//p' "$marker")
    tree=$(sed -n '5s/^source_tree=//p' "$marker")
    inventory_hash=$(sed -n '6s/^tool_inventory_sha256=//p' "$marker")
    config_digest=$(sed -n '7s/^build_config_sha256=//p' "$marker")
    [[ "$commit" =~ ^[0-9a-f]{40}$ && "$toolchain" =~ ^[0-9a-f]{64}$ &&
       "$digest" =~ ^[0-9a-f]{64}$ && "$tree" =~ ^[0-9a-f]{40}$ &&
       "$inventory_hash" =~ ^[0-9a-f]{64}$ &&
       "$config_digest" =~ ^[0-9a-f]{64}$ && "$config_digest" == "$expected_config" &&
       "$tree" == "$expected_tree" && "$commit" == "$expected_commit" &&
       "$toolchain" == "$expected_toolchain" && "$command" == "$aura_web_build_command" &&
       "$digest" == "$expected_digest" && $(wc -l < "$marker") -eq 7 ]] || return 1
    local inventory current_inventory result
    local private_dir="$repo_root/artifacts/aura-web/private-provenance"
    [[ -d "$private_dir" && ! -L "$private_dir" && -O "$private_dir" &&
       "$(aura_web_mode "$private_dir")" == 700 ]] || return 1
    inventory="$private_dir/prebuilt-tools-$commit-$inventory_hash"
    aura_web_private_file "$inventory" || return 1
    [[ "$(sha256sum "$inventory" | cut -d' ' -f1)" == "$inventory_hash" ]] || return 1
    current_inventory=$(mktemp "${TMPDIR:-/tmp}/aura-web-tools.XXXXXX") || return 1
    result=0
    aura_web_selected_tools_match "$inventory" || result=$?
    if [[ "$result" == 0 ]]; then
        bash "$repo_root/scripts/web/dx.sh" --selected-tool-inventory "$current_inventory" >/dev/null 2>&1 || result=1
        cmp -s "$inventory" "$current_inventory" || result=1
    fi
    rm -f "$current_inventory"
    return "$result"
}
