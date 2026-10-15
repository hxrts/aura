#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
runner=$repo_root/scripts/dev/run-os-keychain-diagnostic.sh
scratch=$(mktemp -d)
trap 'rm -rf -- "$scratch"' EXIT
cat >"$scratch/fake-binary" <<EOF
#!/usr/bin/env bash
touch "$scratch/binary-ran"
EOF
chmod 700 "$scratch/fake-binary"

set +e
bash "$runner" --run "$scratch/fake-binary" "$scratch/receipt" >"$scratch/output" 2>&1
status=$?
set -e
[[ $status -eq 64 && ! -e $scratch/binary-ran && ! -e $scratch/receipt ]]

# On an ordinary developer account the marker preflight must refuse before
# executing any supplied binary or allocating a profile/receipt.
if [[ $(uname -s) == Darwin && ! -e /private/var/db/aura/keychain-diagnostic-identity ]]; then
    set +e
    AURA_OS_KEYCHAIN_DIAGNOSTIC=isolated-account bash "$runner" --run "$scratch/fake-binary" "$scratch/receipt" >"$scratch/output" 2>&1
    status=$?
    set -e
    [[ $status -eq 78 && ! -e $scratch/binary-ran && ! -e $scratch/receipt ]]
fi
echo 'OS Keychain diagnostic launcher refuses unsafe local entry before provider access'
