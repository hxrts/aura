#!/usr/bin/env bash
# Diagnostic only: two independent native owners on one host, no build or SSH.
set -euo pipefail
set +x
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/../../.." && pwd -P)
token=${1:-} peer=${2:-}
[[ "$token" =~ ^[a-z0-9][a-z0-9-]*$ && ${#token} -ge 16 && -n "$peer" ]] || {
  echo 'usage: single-host-enrollment.sh <fresh-token> <clean-peer-checkout>' >&2; exit 2;
}
peer=$(cd "$peer" && pwd -P)
[[ "$peer" != "$root" && ! -L "$peer/target" ]] || exit 2
for checkout in "$root" "$peer"; do
  [[ -z "$(git -C "$checkout" status --porcelain)" ]] || {
    echo 'single-host diagnostic requires clean source checkouts' >&2; exit 1;
  }
done
commit=$(git -C "$root" rev-parse HEAD)
[[ "$commit" == "$(git -C "$peer" rev-parse HEAD)" ]] || {
  echo 'single-host diagnostic requires identical source revisions' >&2; exit 1;
}
tool=${AURA_E2E_TOOL_REPL:-$root/target/release/tool_repl}
aura=${AURA_E2E_AURA_BIN:-$root/bin/aura}
[[ -f "$tool" && -x "$tool" && -f "$aura" && -x "$aura" ]] || {
  echo 'prebuilt native binaries are missing' >&2; exit 1;
}
tool_hash=$(shasum -a 256 "$tool" | awk '{print $1}')
aura_hash=$(shasum -a 256 "$aura" | awk '{print $1}')
[[ "$tool_hash" =~ ^[0-9a-f]{64}$ && "$aura_hash" =~ ^[0-9a-f]{64}$ ]] || exit 1
command -v openssl >/dev/null && command -v jq >/dev/null || exit 127
umask 077
local_run="$root/.tmp/e2e/run"
peer_run="$peer/.tmp/e2e/run"
local_provision="$root/.tmp/e2e/single-host-$token"
peer_provision="$peer/.tmp/e2e/single-host-$token"
for path in "$local_provision" "$peer_provision"; do
  [[ ! -e "$path" && ! -L "$path" && ! -L "$(dirname "$path")" ]] || {
    echo 'diagnostic provision already exists or has a linked parent' >&2; exit 1;
  }
  mkdir -p "$(dirname "$path")"
  mkdir -m 700 "$path"
done
auth=$(openssl rand -hex 32)
[[ "$auth" =~ ^[0-9a-f]{64}$ ]] || exit 1
source "$here/render-single-host-config.sh"
render_single_host_config "$root/configs/harness/lan-host-a.toml" "$local_provision/host-a.toml" local "$auth"
render_single_host_config "$peer/configs/harness/lan-host-b.toml" "$peer_provision/host-b.toml" peer "$auth"
unset auth
preflight="$root/.tmp/lan-checklist-$token/paired-preflight.json"
mkdir -m 700 "$(dirname "$preflight")"
host=$(hostname)
jq -cn --arg commit "$commit" --arg token "$token" --arg host "$host" \
  --arg tool_hash "$tool_hash" --arg aura_hash "$aura_hash" '
  {phase:"single-host-preflight",commit:{local:$commit,remote:$commit},run_token:$token,
   cross_host_asserted:false,network_delivery_asserted:false,
   planned_process_identity:{local:{host:$host},remote:{host:$host,actor:"barbara-tui"}},
   artifacts:{tool_repl_sha256:$tool_hash,aura_sha256:$aura_hash},
   broker:{endpoint:"http://127.0.0.1:43190",policy:"development_only_loopback_http",
     recipient_kind:"native"}}' > "$preflight"
chmod 600 "$preflight"
export AURA_E2E_ROOT="$root" AURA_E2E_RUN_DIR="$local_run" AURA_E2E_RUN_TOKEN="$token"
export AURA_E2E_TOOL_REPL="$tool" AURA_E2E_AURA_BIN="$aura"
export AURA_E2E_LOCAL_PEER_ROOT="$peer" AURA_E2E_LOCAL_PEER_RUN_DIR="$peer_run"
export AURA_E2E_LOCAL_CONFIG="$local_provision/host-a.toml"
export AURA_E2E_BOOTSTRAP=paired-enrollment AURA_E2E_REMOTE=''
export AURA_E2E_ENROLLMENT_RECIPIENT=native
source "$here/lib.sh"
local_started=0 peer_started=0 outcome=failed
finish() {
  local status=$? local_disposed=false peer_disposed=false disposal_dir disposal_tmp
  if (( peer_started )); then
    if env AURA_E2E_ROOT="$peer" AURA_E2E_RUN_DIR="$peer_run" AURA_E2E_RUN_TOKEN="$token" \
      "$peer/scripts/harness/lan/drv.sh" finalize "$token" "$outcome"; then
      peer_disposed=true
    else
      status=1
    fi
  fi
  if (( local_started )); then
    if "$AURA_E2E_DRV" finalize "$token" "$outcome"; then local_disposed=true; else status=1; fi
  fi
  disposal_dir="$local_run/private-enrollment-$token"
  if [[ -d "$disposal_dir" && ! -L "$disposal_dir" && -O "$disposal_dir" ]]; then
    [[ ! -e "$disposal_dir/single-host-disposal.json" &&
       ! -L "$disposal_dir/single-host-disposal.json" ]] || exit 1
    disposal_tmp=$(mktemp "$disposal_dir/.disposal.XXXXXX")
    jq -cn --arg commit "$commit" --arg token "$token" --argjson status "$status" \
      --argjson local "$local_disposed" --argjson peer "$peer_disposed" \
      '{schema:1,phase:"single-host-disposal",commit:$commit,run_token:$token,
        final_exit_status:$status,local_owner_disposed:$local,peer_owner_disposed:$peer,
        cross_host_asserted:false}' > "$disposal_tmp"
    chmod 600 "$disposal_tmp"
    mv "$disposal_tmp" "$disposal_dir/single-host-disposal.json"
  fi
  exit "$status"
}
trap finish EXIT
local_started=1
"$AURA_E2E_DRV" start "$local_provision/host-a.toml" >/dev/null
peer_started=1
env AURA_E2E_ROOT="$peer" AURA_E2E_RUN_DIR="$peer_run" AURA_E2E_RUN_TOKEN="$token" \
  AURA_E2E_TOOL_REPL="$tool" AURA_E2E_AURA_BIN="$aura" \
  "$peer/scripts/harness/lan/drv.sh" start "$peer_provision/host-b.toml" >/dev/null
onboard alex-tui Alex >/dev/null
onboard barbara-tui Barbara >/dev/null
bash "$here/paired-enrollment.sh"
outcome=success
