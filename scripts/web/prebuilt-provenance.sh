#!/usr/bin/env bash
# Source-bound proof that the guarded Dioxus build completed on a clean commit.
# Dioxus may reuse byte-identical output without refreshing index.html mtime.

aura_web_clean_commit() {
    local repo_root=$1
    [[ -z "$(git -C "$repo_root" status --porcelain --untracked-files=normal)" ]] || return 1
    git -C "$repo_root" rev-parse --verify HEAD
}

aura_web_bundle_digest() {
    local public_dir=$1
    [[ -d "$public_dir" && ! -L "$public_dir" &&
       -f "$public_dir/index.html" && ! -L "$public_dir/index.html" ]] || return 1
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

aura_web_build_args=(build --release --profile wasm --platform web --package aura-web --bin aura-web --features web,harness)
aura_web_build_command="dx ${aura_web_build_args[*]}"

aura_web_write_prebuilt_provenance() {
    local repo_root=$1 public_dir=$2 marker commit digest toolchain scratch
    marker="$public_dir/.aura-prebuilt-provenance"
    [[ ! -L "$marker" ]] || return 1
    # Ordinary dirty development builds remain usable, but cannot claim a
    # fixed-source prebuilt bundle for a LAN harness run.
    commit=$(aura_web_clean_commit "$repo_root") || return 0
    digest=$(aura_web_bundle_digest "$public_dir") || return 1
    toolchain=$(aura_web_toolchain_lock_digest "$repo_root") || return 1
    [[ "$commit" =~ ^[0-9a-f]{40}$ && "$digest" =~ ^[0-9a-f]{64}$ &&
       "$toolchain" =~ ^[0-9a-f]{64}$ ]] || return 1
    scratch=$(mktemp "$public_dir/.aura-prebuilt-provenance.XXXXXX") || return 1
    printf 'commit=%s\ntoolchain_lock_sha256=%s\nbuild_command=%s\nbundle_sha256=%s\n' \
        "$commit" "$toolchain" "$aura_web_build_command" "$digest" > "$scratch"
    chmod 600 "$scratch"
    mv -f "$scratch" "$marker"
}

aura_web_prebuilt_provenance_matches() {
    local repo_root=$1 public_dir=$2 marker commit digest toolchain command expected_commit expected_digest expected_toolchain
    marker="$public_dir/.aura-prebuilt-provenance"
    [[ -f "$marker" && ! -L "$marker" ]] || return 1
    expected_commit=$(aura_web_clean_commit "$repo_root") || return 1
    expected_digest=$(aura_web_bundle_digest "$public_dir") || return 1
    expected_toolchain=$(aura_web_toolchain_lock_digest "$repo_root") || return 1
    commit=$(sed -n '1s/^commit=//p' "$marker")
    toolchain=$(sed -n '2s/^toolchain_lock_sha256=//p' "$marker")
    command=$(sed -n '3s/^build_command=//p' "$marker")
    digest=$(sed -n '4s/^bundle_sha256=//p' "$marker")
    [[ "$commit" =~ ^[0-9a-f]{40}$ && "$toolchain" =~ ^[0-9a-f]{64}$ &&
       "$digest" =~ ^[0-9a-f]{64}$ && "$commit" == "$expected_commit" &&
       "$toolchain" == "$expected_toolchain" && "$command" == "$aura_web_build_command" &&
       "$digest" == "$expected_digest" && $(wc -l < "$marker") -eq 4 ]]
}
