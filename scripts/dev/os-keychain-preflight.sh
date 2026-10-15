#!/usr/bin/env bash
# Fail-closed identity preflight for a future real macOS Keychain diagnostic.
# This script never opens or changes a Keychain item.
set -euo pipefail

check_component() {
    local path=$1 owner=$2 kind=$3 mode actual_owner
    [[ ! -L $path ]] || return 1
    if [[ $kind == dir ]]; then
        [[ -d $path ]] || return 1
    else
        [[ -f $path ]] || return 1
    fi
    actual_owner=$(/usr/bin/stat -f '%u' "$path") || return 1
    mode=$(/usr/bin/stat -f '%Lp' "$path") || return 1
    [[ $actual_owner == "$owner" && $mode =~ ^[0-7]{3,4}$ ]] || return 1
    (( (8#$mode & 022) == 0 ))
}

check_identity_marker() {
    local root=$1 owner=$2 current_uid=$3 marker component line index=0
    local isolated_uid= developer_uid=
    marker=$root/var/db/aura/keychain-diagnostic-identity
    check_component "$root" "$owner" dir || return 1
    for component in "$root/var" "$root/var/db" "$root/var/db/aura"; do
        check_component "$component" "$owner" dir || return 1
    done
    check_component "$marker" "$owner" file || return 1
    [[ $(/usr/bin/stat -f '%z' "$marker") -le 128 ]] || return 1
    while IFS= read -r line || [[ -n $line ]]; do
        case $index in
            0) [[ $line =~ ^isolated_uid=([0-9]+)$ ]] || return 1
               isolated_uid=${BASH_REMATCH[1]} ;;
            1) [[ $line =~ ^developer_uid=([0-9]+)$ ]] || return 1
               developer_uid=${BASH_REMATCH[1]} ;;
            *) return 1 ;;
        esac
        index=$((index + 1))
    done < "$marker"
    [[ $index -eq 2 && $isolated_uid != 0 && $developer_uid != "$isolated_uid" && $current_uid == "$isolated_uid" ]]
}

if [[ ${BASH_SOURCE[0]} != "$0" ]]; then
    return 0
fi

if [[ $# -ne 1 || $1 != --preflight ]]; then
    echo 'usage: os-keychain-preflight.sh --preflight' >&2
    exit 64
fi
if [[ $(/usr/bin/uname -s) != Darwin ]]; then
    echo 'OS Keychain diagnostic requires macOS' >&2
    exit 78
fi
if ! check_identity_marker /private 0 "$(/usr/bin/id -u)"; then
    echo 'OS Keychain diagnostic refused: isolated account marker is absent or unsafe' >&2
    exit 78
fi
echo 'Isolated account preflight passed; no Keychain access or durability proof performed.'
