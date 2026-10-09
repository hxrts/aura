#!/usr/bin/env bash
# Finalize only the exact run this batch launched; preserve unknown ownership.
set -euo pipefail
[[ $# == 4 ]] || { echo 'usage: finish-batch.sh host-or-empty checkout token success|failed' >&2; exit 2; }
host=$1 checkout=$2 token=$3 outcome=$4
[[ "$token" =~ ^[a-z0-9-]{16,}$ && ( "$outcome" == success || "$outcome" == failed ) ]] || exit 2
finish_script=$(cat <<'SCRIPT'
set -euo pipefail
cd "$1"
export AURA_E2E_ROOT="$PWD"
# The pinned driver validates the expected token inside its lifecycle owner,
# after environment bootstrap, and retains custody through stop and finish.
bash scripts/harness/lan/drv.sh finalize "$2" "$3"
SCRIPT
)
if [[ -n "$host" ]]; then
  printf -v remote_command 'bash -s -- %q %q %q' "$checkout" "$token" "$outcome"
  ssh -o BatchMode=yes -o ConnectTimeout=15 "$host" "$remote_command" <<< "$finish_script"
else
  bash -s -- "$checkout" "$token" "$outcome" <<< "$finish_script"
fi
