#!/usr/bin/env bash
# Prepared task463 owner. Transport authentication belongs to ship.sh's SSH.
# Source after prebuilt-provenance.sh; all evidence stays outside public assets.
aura_web_transfer_payload_matches() {
    local repo_root=$1 public_dir=$2 evidence=$3 marker inventory commit tree bundle lock platform
    aura_web_private_file "$evidence" || return 1
    jq -e 'type=="object" and keys==["marker","platform","schema","tools"] and
      .schema==1 and (.marker|type=="string") and (.tools|type=="string") and
      (.platform|type=="string" and length>0)' "$evidence" >/dev/null || return 1
    platform=$(uname -sm) || return 1
    [[ -n "$platform" && $(jq -r .platform "$evidence") == "$platform" ]] || return 1
    marker=$(jq -r .marker "$evidence") || return 1
    inventory=$(jq -r .tools "$evidence") || return 1
    # Preserve the original exact producer marker. No consumer configuration or
    # current tool path may be relabelled as the producer's build inputs.
    [[ $(jq -jr .marker "$evidence" | sha256sum | cut -d' ' -f1) == \
       "$(sha256sum "$public_dir/.aura-prebuilt-provenance" | cut -d' ' -f1)" ]] || return 1
    [[ $(printf '%s\n' "$marker" | wc -l) -eq 7 ]] || return 1
    commit=$(printf '%s\n' "$marker" | sed -n '1s/^commit=//p')
    lock=$(printf '%s\n' "$marker" | sed -n '2s/^toolchain_lock_sha256=//p')
    [[ $(printf '%s\n' "$marker" | sed -n '3s/^build_command=//p') == "$aura_web_build_command" ]] || return 1
    bundle=$(printf '%s\n' "$marker" | sed -n '4s/^bundle_sha256=//p')
    tree=$(printf '%s\n' "$marker" | sed -n '5s/^source_tree=//p')
    [[ "$commit" =~ ^[0-9a-f]{40}$ && "$tree" =~ ^[0-9a-f]{40}$ &&
       "$lock" =~ ^[0-9a-f]{64}$ && "$bundle" =~ ^[0-9a-f]{64}$ &&
       $(printf '%s\n' "$marker" | sed -n '7s/^build_config_sha256=//p') =~ ^[0-9a-f]{64}$ ]] || return 1
    [[ "$commit" == "$(aura_web_clean_commit "$repo_root")" &&
       "$tree" == "$(aura_web_clean_tree "$repo_root")" &&
       "$lock" == "$(aura_web_toolchain_lock_digest "$repo_root")" &&
       "$bundle" == "$(aura_web_bundle_digest "$public_dir")" ]] || return 1
    [[ $(jq -jr .tools "$evidence" | sha256sum | cut -d' ' -f1) == \
       "$(printf '%s\n' "$marker" | sed -n '6s/^tool_inventory_sha256=//p')" ]] || return 1
    # Validate inventory shape without requiring unavailable producer executables.
    printf '%s\n' "$inventory" | awk -F '|' '
      BEGIN { split("node npm cargo rustc dx wasm-bindgen wasm-opt binaryen-lib-tree", names, " ") }
      NF!=3 || $1!=names[NR] || $2!~/^\// || length($3)!=64 || $3~/[^0-9a-f]/ {bad=1}
      END {exit bad || NR!=8}' || return 1
}

aura_web_export_transfer() {
    local repo_root=$1 public_dir=$2 output=$3 marker inventory_hash commit inventory platform
    aura_web_prebuilt_provenance_matches "$repo_root" "$public_dir" || return 1
    aura_web_private_file "$output" || return 1
    [[ ! -s "$output" ]] || return 1
    marker="$public_dir/.aura-prebuilt-provenance"
    commit=$(sed -n '1s/^commit=//p' "$marker")
    inventory_hash=$(sed -n '6s/^tool_inventory_sha256=//p' "$marker")
    inventory="$repo_root/artifacts/aura-web/private-provenance/prebuilt-tools-$commit-$inventory_hash"
    platform=$(uname -sm) || return 1
    [[ -n "$platform" ]] || return 1
    jq -n --rawfile marker "$marker" --rawfile tools "$inventory" --arg platform "$platform" \
      '{schema:1,platform:$platform,marker:$marker,tools:$tools}' > "$output" || return 1
    aura_web_transfer_payload_matches "$repo_root" "$public_dir" "$output"
}

aura_web_transfer_private_dir() {
    local repo_root=$1 create=${2:-0} directory="$1/artifacts/aura-web/transferred-provenance" component current mode
    [[ "$repo_root" == "$(cd "$repo_root" && pwd -P)" && -O "$repo_root" ]] || return 1
    # Reject links in every existing parent, including the checkout's own path.
    current=''
    local -a components=()
    IFS=/ read -ra components <<< "$directory"
    for component in "${components[@]}"; do
        [[ -n "$component" ]] || continue
        [[ "$component" != . && "$component" != .. ]] || return 1
        current="$current/$component"
        [[ ! -L "$current" ]] || return 1
        if [[ "$current" == "$repo_root" || "$current" == "$repo_root/"* ]]; then
            if [[ -e "$current" ]]; then
                [[ -d "$current" && -O "$current" ]] || return 1
                mode=$(aura_web_mode "$current") || return 1
                (( (8#$mode & 022) == 0 )) || return 1
            fi
        fi
    done
    if [[ ! -e "$directory" ]]; then
        [[ "$create" == 1 ]] || return 1
        mkdir -p "$repo_root/artifacts/aura-web" || return 1
        mkdir -m 700 "$directory" || return 1
    fi
    [[ -d "$directory" && -O "$directory" && $(aura_web_mode "$directory") == 700 ]] || return 1
    printf '%s\n' "$directory"
}

# stdin must arrive from the authenticated ship transport; expected_sha is
# selected by the producer after strict local build validation, never by stdin.
aura_web_install_transfer() (
    local repo_root=$1 public_dir=$2 expected_sha=$3 directory stage='' selection='' actual root record_key ready
    umask 077
    [[ "$expected_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
    aura_web_private_file "$public_dir/.aura-prebuilt-provenance" || return 1
    directory=$(aura_web_transfer_private_dir "$repo_root" 1) || return 1
    [[ ! -L "$directory/.install.lock" ]] || return 1
    if [[ -e "$directory/.install.lock" ]]; then
        aura_web_private_file "$directory/.install.lock" || return 1
    fi
    # This function owns a subshell, so its fixed descriptor cannot replace a
    # lifecycle descriptor held by the caller. Match the portable driver lane.
    exec 9> "$directory/.install.lock" || return 1
    flock -n 9 || return 1
    trap '[[ -z "$stage" ]] || rm -rf -- "$stage"; [[ -z "$selection" ]] || rm -f -- "$selection"' EXIT
    [[ ! -L "$directory/selected" ]] || return 1
    if [[ -e "$directory/selected" ]]; then
        aura_web_private_file "$directory/selected" || return 1
    fi
    record_key=$(sha256sum "$public_dir/.aura-prebuilt-provenance" | cut -d' ' -f1) || return 1
    ready="$directory/ready-$record_key"
    stage=$(mktemp -d "$directory/pending.XXXXXX") || return 1
    chmod 700 "$stage" || { rmdir "$stage"; return 1; }
    # Never consume an unbounded remote stream before validating its digest.
    head -c 1048577 > "$stage/evidence.json" || return 1
    [[ $(wc -c < "$stage/evidence.json") -le 1048576 ]] || return 1
    actual=$(sha256sum "$stage/evidence.json" | cut -d' ' -f1) || { rm -rf "$stage"; return 1; }
    if [[ "$actual" != "$expected_sha" ]] || ! aura_web_transfer_payload_matches "$repo_root" "$public_dir" "$stage/evidence.json"; then
        rm -rf "$stage"; return 1
    fi
    root=$(cd "$repo_root" && pwd -P) || { rm -rf "$stage"; return 1; }
    (umask 077; printf '%s\n%s\n' "$root" "$expected_sha" > "$stage/admission") || { rm -rf "$stage"; return 1; }
    if [[ -e "$ready" || -L "$ready" ]]; then
        aura_web_transferred_provenance_matches "$repo_root" "$public_dir" || return 1
        cmp -s "$stage/evidence.json" "$ready/evidence.json" &&
            cmp -s "$stage/admission" "$ready/admission" || return 1
    else
        mv "$stage" "$ready" || return 1
        stage=''
    fi
    # Publish explicit consumer selection only after immutable evidence exists.
    # A retry also completes selection after interruption following ready rename.
    selection=$(mktemp "$directory/selection.XXXXXX") || return 1
    printf '%s\n%s\n%s\n' "$root" "$record_key" "$expected_sha" > "$selection" || return 1
    mv -f "$selection" "$directory/selected" || return 1
    selection=''
    aura_web_admitted_prebuilt_matches "$repo_root" "$public_dir"
)

aura_web_transferred_provenance_matches() {
    local repo_root=$1 public_dir=$2 directory root digest record_key ready
    aura_web_private_file "$public_dir/.aura-prebuilt-provenance" || return 1
    directory=$(aura_web_transfer_private_dir "$repo_root") || return 1
    record_key=$(sha256sum "$public_dir/.aura-prebuilt-provenance" | cut -d' ' -f1) || return 1
    ready="$directory/ready-$record_key"
    [[ -d "$ready" && ! -L "$ready" && -O "$ready" &&
       $(aura_web_mode "$ready") == 700 ]] || return 1
    aura_web_private_file "$ready/admission" || return 1
    aura_web_private_file "$ready/evidence.json" || return 1
    [[ $(wc -l < "$ready/admission") -eq 2 ]] || return 1
    root=$(sed -n 1p "$ready/admission")
    digest=$(sed -n 2p "$ready/admission")
    [[ "$root" == "$(cd "$repo_root" && pwd -P)" && "$digest" =~ ^[0-9a-f]{64}$ &&
       "$digest" == "$(sha256sum "$ready/evidence.json" | cut -d' ' -f1)" ]] || return 1
    aura_web_transfer_payload_matches "$repo_root" "$public_dir" "$ready/evidence.json"
}

# The presence of transferred selection is decisive. An invalid selection or
# missing retained evidence cannot be repaired with local producer fallback.
aura_web_admitted_prebuilt_matches() {
    local repo_root=$1 public_dir=$2 directory="$1/artifacts/aura-web/transferred-provenance" selected key digest
    selected="$directory/selected"
    if [[ -e "$directory" || -L "$directory" ]]; then
        aura_web_transfer_private_dir "$repo_root" >/dev/null || return 1
        if [[ -e "$selected" || -L "$selected" ]]; then
            aura_web_private_file "$selected" || return 1
            [[ $(wc -l < "$selected") -eq 3 && $(sed -n 1p "$selected") == "$(cd "$repo_root" && pwd -P)" ]] || return 1
            aura_web_private_file "$public_dir/.aura-prebuilt-provenance" || return 1
            key=$(sha256sum "$public_dir/.aura-prebuilt-provenance" | cut -d' ' -f1) || return 1
            [[ $(sed -n 2p "$selected") == "$key" ]] || return 1
            digest=$(sed -n 3p "$selected")
            [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || return 1
            aura_web_transferred_provenance_matches "$repo_root" "$public_dir" || return 1
            [[ $(sed -n 2p "$directory/ready-$key/admission") == "$digest" ]]
            return $?
        fi
    fi
    aura_web_prebuilt_provenance_matches "$repo_root" "$public_dir"
}

# Only successful strict local-producer proof can deliberately retire transfer
# selection. Retain immutable historical transfer evidence for diagnosis.
aura_web_select_local_producer() (
    local repo_root=$1 public_dir=$2 directory="$1/artifacts/aura-web/transferred-provenance"
    aura_web_prebuilt_provenance_matches "$repo_root" "$public_dir" || return 1
    [[ -e "$directory" || -L "$directory" ]] || return 0
    aura_web_transfer_private_dir "$repo_root" >/dev/null || return 1
    [[ ! -L "$directory/.install.lock" ]] || return 1
    if [[ -e "$directory/.install.lock" ]]; then
        aura_web_private_file "$directory/.install.lock" || return 1
    fi
    umask 077
    exec 9> "$directory/.install.lock" || return 1
    flock -n 9 || return 1
    aura_web_prebuilt_provenance_matches "$repo_root" "$public_dir" || return 1
    if [[ -e "$directory/selected" || -L "$directory/selected" ]]; then
        aura_web_private_file "$directory/selected" || return 1
        rm -- "$directory/selected" || return 1
    fi
)

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    set -euo pipefail
    transfer_script_dir=$(cd "$(dirname "$0")" && pwd -P)
    source "$transfer_script_dir/prebuilt-provenance.sh"
    case "${1:-}" in
        export)
            [[ $# == 4 ]] || exit 2
            aura_web_export_transfer "$2" "$3" "$4"
            ;;
        install)
            [[ $# == 4 ]] || exit 2
            aura_web_install_transfer "$2" "$3" "$4"
            ;;
        verify)
            [[ $# == 3 ]] || exit 2
            aura_web_admitted_prebuilt_matches "$2" "$3"
            ;;
        *) echo 'usage: prebuilt-transfer.sh export|install ROOT PUBLIC FILE_OR_DIGEST; verify ROOT PUBLIC' >&2; exit 2 ;;
    esac
fi
