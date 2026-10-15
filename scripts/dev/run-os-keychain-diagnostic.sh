#!/usr/bin/env bash
# Opt-in, isolated-account diagnostic. Never run with the developer login UID.
set -euo pipefail

if [[ $# -ne 3 || $1 != --run || ${AURA_OS_KEYCHAIN_DIAGNOSTIC:-} != isolated-account ]]; then
    echo 'usage: AURA_OS_KEYCHAIN_DIAGNOSTIC=isolated-account run-os-keychain-diagnostic.sh --run <prebuilt-binary> <receipt-dir>' >&2
    exit 64
fi
binary=$2
receipt_dir=$3

# Do this before even checking the binary. The binary repeats the same root-
# owned marker/UID check, so skipping this wrapper cannot reach the Keychain.
bash "$(dirname "$0")/os-keychain-preflight.sh" --preflight >/dev/null
[[ -f $binary && -x $binary && ! -L $binary ]] || { echo 'diagnostic binary unavailable' >&2; exit 78; }
[[ ! -e $receipt_dir ]] || { echo 'receipt directory already exists' >&2; exit 78; }
umask 077
mkdir -m 700 -- "$receipt_dir"
profile=$(mktemp -d "${TMPDIR:-/private/tmp}/aura-os-keychain-profile.XXXXXXXX")
chmod 700 "$profile"
trap 'rm -rf -- "$profile"' EXIT

set +e
"$binary" --profile "$profile" >"$receipt_dir/result.json" 2>"$receipt_dir/diagnostic.log"
status=$?
set -e
printf '%s\n' "$status" >"$receipt_dir/exit-status"
# The binary emits only typed class names; detailed stderr remains private.
cat "$receipt_dir/result.json"
exit "$status"
