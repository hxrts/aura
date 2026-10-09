#!/usr/bin/env bash
# Run an explicit LAN checklist command after a verified fixed-commit ship.
# Usage: batch.sh <fresh-token> -- <command> [arguments...]
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$here/../../.."
[[ $# -ge 3 && "$1" =~ ^[a-z0-9][a-z0-9-]*$ && ${#1} -ge 16 && "$2" == -- ]] || { echo 'usage: batch.sh <fresh-token> -- <command> [arguments...]' >&2; exit 2; }
export AURA_E2E_ROOT="$PWD"
export AURA_E2E_RUN_TOKEN=$1
shift 2
checklist_command=("$@")
source scripts/harness/lan/env.sh
[[ -n "$AURA_E2E_REMOTE" ]] || { echo 'set AURA_E2E_REMOTE for the other host' >&2; exit 2; }
if [[ -z "${IN_NIX_SHELL:-}" ]]; then
  exec "${AURA_NIX_BIN:-/nix/var/nix/profiles/default/bin/nix}" develop "$PWD" --command bash "$here/batch.sh" "$AURA_E2E_RUN_TOKEN" -- "${checklist_command[@]}"
fi
checkpoint=$(git rev-parse HEAD)
[[ -z "$(git status --porcelain --untracked-files=no)" ]] || { echo 'LAN checkpoint is dirty' >&2; exit 1; }
[[ $(cat ".tmp/main-smoke-$checkpoint/validated-commit") == "$checkpoint" ]] || { echo 'matching complete smoke evidence missing' >&2; exit 1; }
quoted_root=${AURA_E2E_REMOTE_ROOT//\'/\'\\\'\'}
remote_checkpoint=$(ssh -o BatchMode=yes -o ConnectTimeout=15 "$AURA_E2E_REMOTE" "cd '$quoted_root' && test -z \"\$(git status --porcelain --untracked-files=no)\" && git rev-parse HEAD")
[[ "$remote_checkpoint" == "$checkpoint" ]] || { echo 'remote checkpoint differs; finish shipping first' >&2; exit 1; }
evidence="$PWD/.tmp/lan-checklist-$AURA_E2E_RUN_TOKEN"
mkdir "$evidence"
printf '%s\n' "$checkpoint" > "$evidence/commit"
printf '%s\0' "${checklist_command[@]}" > "$evidence/checklist-command"
result=0
finalized=false
batch_pid=''
cleanup_run() {
  local incoming=$1 outcome=failed cleanup_result=0
  [[ "$finalized" == false ]] || return 0
  finalized=true
  (( incoming == 0 )) || result=$incoming
  (( result != 0 )) || outcome=success
  # Both hosts are attempted independently. Token and tracked process checks
  # must succeed before signalling or finalizing retained evidence.
  timeout 120 bash "$here/finish-batch.sh" '' "$PWD" "$AURA_E2E_RUN_TOKEN" "$outcome" > "$evidence/local-shutdown.log" 2>&1 || cleanup_result=1
  timeout 120 bash "$here/finish-batch.sh" "$AURA_E2E_REMOTE" "$AURA_E2E_REMOTE_ROOT" "$AURA_E2E_RUN_TOKEN" "$outcome" > "$evidence/remote-shutdown.log" 2>&1 || cleanup_result=1
  printf '%s\n' "$cleanup_result" > "$evidence/shutdown-exit-status"
  (( cleanup_result == 0 )) || result=1
  printf '%s\n' "$result" > "$evidence/exit-status"
}
trap 'cleanup_run "$?"' EXIT
cancel_batch() {
  local status=$1
  trap - INT TERM
  if [[ -n "$batch_pid" ]]; then
    # This direct child is the timeout owner created below. Its TERM handler
    # cancels its owned command tree; wait for termination before finalizing.
    kill -TERM "$batch_pid" 2>/dev/null || true
    wait "$batch_pid" || true
    batch_pid=''
  fi
  exit "$status"
}
trap 'cancel_batch 130' INT
trap 'cancel_batch 143' TERM
timeout 1500 bash -c '
  set -euo pipefail
  bash scripts/harness/lan/fresh.sh "$AURA_E2E_RUN_TOKEN"
  exec "$@"
' lan-checklist "${checklist_command[@]}" > "$evidence/run.log" 2>&1 &
batch_pid=$!
wait "$batch_pid" || result=$?
batch_pid=''
[[ $(git rev-parse HEAD) == "$checkpoint" && -z "$(git status --porcelain --untracked-files=no)" ]] || { echo 'checkpoint changed during LAN run' >&2; result=1; }
cleanup_run "$result"
trap - EXIT INT TERM
cat "$evidence/run.log"
echo "LAN evidence: $evidence (exit $result)"
exit "$result"
