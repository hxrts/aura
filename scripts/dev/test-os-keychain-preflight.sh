#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
source "$script_dir/os-keychain-preflight.sh"

fixture=$(mktemp -d "${TMPDIR:-/private/tmp}/aura-keychain-preflight.XXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
mkdir -p "$fixture/var/db/aura"
marker=$fixture/var/db/aura/keychain-diagnostic-identity
owner=$(/usr/bin/id -u)
other=$((owner + 1))
printf 'isolated_uid=%s\ndeveloper_uid=%s\n' "$owner" "$other" > "$marker"
chmod 600 "$marker"

check_identity_marker "$fixture" "$owner" "$owner"
if check_identity_marker "$fixture" "$other" "$owner"; then
    echo 'foreign marker owner passed' >&2; exit 1
fi
if check_identity_marker "$fixture" "$owner" "$other"; then
    echo 'foreign current UID passed' >&2; exit 1
fi
printf 'isolated_uid=%s\ndeveloper_uid=%s\n' "$owner" "$owner" > "$marker"
if check_identity_marker "$fixture" "$owner" "$owner"; then
    echo 'developer UID passed' >&2; exit 1
fi
printf 'isolated_uid=%s\ndeveloper_uid=%s\n' "$owner" "$other" > "$marker"
chmod 666 "$marker"
if check_identity_marker "$fixture" "$owner" "$owner"; then
    echo 'writable marker passed' >&2; exit 1
fi
chmod 600 "$marker"
chmod 777 "$fixture/var/db/aura"
if check_identity_marker "$fixture" "$owner" "$owner"; then
    echo 'writable ancestor passed' >&2; exit 1
fi
chmod 700 "$fixture/var/db/aura"
printf 'malformed\n' > "$marker"
if check_identity_marker "$fixture" "$owner" "$owner"; then
    echo 'malformed marker passed' >&2; exit 1
fi
printf 'isolated_uid=%s\ndeveloper_uid=%s\n' "$owner" "$other" > "$marker"
printf '%0130d' 0 >> "$marker"
if check_identity_marker "$fixture" "$owner" "$owner"; then
    echo 'oversized marker passed' >&2; exit 1
fi
printf 'isolated_uid=%s\ndeveloper_uid=%s\n' "$owner" "$other" > "$marker"
mv "$fixture/var/db/aura" "$fixture/var/db/aura.original"
ln -s aura.original "$fixture/var/db/aura"
if check_identity_marker "$fixture" "$owner" "$owner"; then
    echo 'symlink ancestor passed' >&2; exit 1
fi
rm "$fixture/var/db/aura"
mv "$fixture/var/db/aura.original" "$fixture/var/db/aura"
mv "$marker" "$marker.original"
ln -s "$marker.original" "$marker"
if check_identity_marker "$fixture" "$owner" "$owner"; then
    echo 'symlink marker passed' >&2; exit 1
fi
rm "$marker"
if check_identity_marker "$fixture" "$owner" "$owner"; then
    echo 'missing marker passed' >&2; exit 1
fi

echo 'OS Keychain preflight fixture: safe identity and ten refusals passed'
