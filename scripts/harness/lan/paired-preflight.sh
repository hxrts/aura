#!/usr/bin/env bash
# Read-only fixed-commit paired LAN admission; write one redacted owner file
# before batch.sh starts either host's REPL.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$here/env.sh"
output=${1:-}
[[ "${AURA_E2E_BOOTSTRAP:-}" == paired-enrollment && -n "$output" ]] || exit 2
[[ "$AURA_E2E_RUN_TOKEN" =~ ^[a-z0-9][a-z0-9-]{15,127}$ ]] || exit 2
[[ -n "$AURA_E2E_REMOTE" ]] || exit 2
recipient_kind=${AURA_E2E_ENROLLMENT_RECIPIENT:-native}
[[ "$recipient_kind" == native || "$recipient_kind" == browser ]] || exit 2
recipient_actor=$(if [[ "$recipient_kind" == browser ]]; then echo barbara-web; else echo barbara-tui; fi)
[[ ! -e "$output" && ! -L "$output" && -d "$(dirname "$output")" ]] || exit 2

commit=$(git -C "$AURA_E2E_ROOT" rev-parse HEAD)
smoke_file="$AURA_E2E_ROOT/.tmp/main-smoke-$commit/validated-commit"
[[ -f "$smoke_file" && $(cat "$smoke_file") == "$commit" ]] || {
  echo 'paired preflight requires matching local smoke receipt' >&2; exit 1;
}
web_index=${AURA_E2E_WEB_INDEX:-$AURA_E2E_ROOT/target/dx/aura-web/release/web/public/index.html}
for artifact in "$AURA_E2E_AURA_BIN" "$AURA_E2E_TOOL_REPL" "$web_index"; do
  [[ -f "$artifact" && ! -L "$artifact" ]] || {
    echo 'paired preflight requires fixed local artifacts' >&2; exit 1;
  }
done
local_aura=$(sha256sum "$AURA_E2E_AURA_BIN" | cut -d' ' -f1)
local_repl=$(sha256sum "$AURA_E2E_TOOL_REPL" | cut -d' ' -f1)
local_web=$(sha256sum "$web_index" | cut -d' ' -f1)
smoke_hash=$(sha256sum "$smoke_file" | cut -d' ' -f1)

local_config=${AURA_E2E_LOCAL_CONFIG:-}
remote_config=${AURA_E2E_REMOTE_CONFIG:-}
[[ -f "$local_config" && ! -L "$local_config" && -n "$remote_config" ]] || {
  echo 'paired preflight requires generated broker configs' >&2; exit 1;
}
[[ "$local_config" == *"enrollment-provision-$AURA_E2E_RUN_TOKEN/host-a.toml" &&
   "$remote_config" == ".tmp/e2e/enrollment-provision-$AURA_E2E_RUN_TOKEN/host-b.toml" ]] || {
  echo 'paired preflight config paths do not match the fresh run token' >&2; exit 1;
}
local_config_mode=$(stat -c '%a' "$local_config" 2>/dev/null || stat -f '%Lp' "$local_config")
[[ -O "$local_config" && "$local_config_mode" == 600 ]] || {
  echo 'paired preflight local broker config is not owner-only' >&2; exit 1;
}
broker_bind=$(sed -n 's/^[[:space:]]*"AURA_BOOTSTRAP_BROKER_BIND=\([^"]*\)",/\1/p' "$local_config")
[[ "$broker_bind" =~ ^[0-9.]+:[0-9]+$ && "$broker_bind" != 127.* &&
   $(rg -c '^[[:space:]]*"AURA_BOOTSTRAP_BROKER_ALLOW_LAN_BIND=true",$' "$local_config") == 1 ]] || {
  echo 'paired preflight requires an explicit development-only LAN broker bind' >&2; exit 1;
}

quoted_root=${AURA_E2E_REMOTE_ROOT//\'/\'\\\'\'}
printf -v quoted_config '%q' "$remote_config"
remote_nix=${AURA_E2E_REMOTE_NIX:-/nix/var/nix/profiles/default/bin/nix}
printf -v quoted_nix '%q' "$remote_nix"
remote_config_mode=$(ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
  "cd '$quoted_root' && test -f $quoted_config && test ! -L $quoted_config && $quoted_nix develop --command stat -c '%a' $quoted_config")
[[ "$remote_config_mode" == 600 ]] || {
  echo 'paired preflight remote broker config is not owner-only' >&2; exit 1;
}
remote_commit=$(ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "cd '$quoted_root' && git rev-parse HEAD")
[[ "$remote_commit" == "$commit" ]] || { echo 'paired preflight remote commit differs' >&2; exit 1; }
remote_host=$(ssh -o BatchMode=yes "$AURA_E2E_REMOTE" 'hostname')
local_host=$(hostname)
remote_hash_text=$(ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
  "cd '$quoted_root' && $quoted_nix develop --command sha256sum bin/aura target/release/tool_repl target/dx/aura-web/release/web/public/index.html")
[[ $(printf '%s\n' "$remote_hash_text" | wc -l | tr -d ' ') == 3 ]] || {
  echo 'paired preflight remote artifact inventory differs' >&2; exit 1;
}
remote_aura=$(sed -n '1s/ .*//p' <<< "$remote_hash_text")
remote_repl=$(sed -n '2s/ .*//p' <<< "$remote_hash_text")
remote_web=$(sed -n '3s/ .*//p' <<< "$remote_hash_text")
for hash in "$local_aura" "$local_repl" "$local_web" "$remote_aura" "$remote_repl" "$remote_web"; do
  [[ "$hash" =~ ^[0-9a-f]{64}$ ]] || { echo 'paired preflight artifact hash is invalid' >&2; exit 1; }
done
[[ "$local_aura" == "$remote_aura" && "$local_repl" == "$remote_repl" &&
   "$local_web" == "$remote_web" ]] || {
  echo 'paired preflight remote artifacts differ' >&2; exit 1;
}
remote_url=$(ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
  "cd '$quoted_root' && sed -n 's/^[[:space:]]*\"$(if [[ "$recipient_kind" == browser ]]; then echo AURA_HARNESS_BROWSER_BROKER_URL; else echo AURA_BOOTSTRAP_BROKER_URL; fi)=\\([^\"]*\\)\",/\\1/p' $quoted_config")
expected_remote_url="http://$broker_bind"
if [[ "$recipient_kind" == browser ]]; then
  relay_port=${AURA_E2E_BROWSER_BROKER_RELAY_PORT:-43191}
  [[ "$relay_port" =~ ^[0-9]+$ ]] && (( relay_port > 1023 && relay_port < 65536 )) || exit 2
  expected_remote_url="http://127.0.0.1:$relay_port"
  local_auth_hash=$(sed -n 's/^[[:space:]]*"AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=\([0-9a-f]\{64\}\)",/\1/p' "$local_config" | sha256sum | cut -d' ' -f1)
  remote_auth_hash=$(ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
    "cd '$quoted_root' && sed -n 's/^[[:space:]]*\"AURA_HARNESS_BROWSER_BROKER_AUTH=\\([0-9a-f]\\{64\\}\\)\",/\\1/p' $quoted_config | sha256sum | cut -d' ' -f1")
  [[ "$local_auth_hash" == "$remote_auth_hash" ]] || {
    echo 'paired preflight browser broker bearer differs' >&2; exit 1;
  }
fi
[[ $(printf '%s\n' "$remote_url" | sort -u) == "$expected_remote_url" ]] || {
  echo 'paired preflight broker endpoint differs across hosts' >&2; exit 1;
}

scratch=$(mktemp "$(dirname "$output")/.paired-preflight.XXXXXX")
jq -cn --arg commit "$commit" --arg smoke "$smoke_file" --arg smoke_hash "$smoke_hash" \
  --arg token "$AURA_E2E_RUN_TOKEN" --arg local_host "$local_host" --arg remote_host "$remote_host" \
  --arg remote "$AURA_E2E_REMOTE" --arg broker "http://$broker_bind" \
  --arg recipient "$recipient_actor" --arg recipient_kind "$recipient_kind" \
  --arg local_config "$local_config" --arg remote_config "$remote_config" \
  --arg aura "$local_aura" --arg repl "$local_repl" --arg web "$local_web" \
  --arg remote_aura "$remote_aura" --arg remote_repl "$remote_repl" --arg remote_web "$remote_web" '
  {schema:1,phase:"preflight",commit:{local:$commit,remote:$commit},run_token:$token,
   local_smoke_receipt:{path:$smoke,sha256:$smoke_hash,validated_commit:$commit},
   artifact_sha256:{local:{aura:$aura,tool_repl:$repl,web_index:$web},
                    remote:{aura:$remote_aura,tool_repl:$remote_repl,web_index:$remote_web}},
   planned_process_identity:{local:{host:$local_host,actor:"alex-tui",config:$local_config,
      executable_sha256:$repl,pid_birth:"pending_start"},
      remote:{host:$remote_host,ssh_destination:$remote,actor:$recipient,config:$remote_config,
      executable_sha256:$repl,pid_birth:"pending_start"}},
   broker:{endpoint:$broker,policy:"development_only_lan_http",
      recipient_kind:$recipient_kind},
   inventory:["signed_setup_export","first_authenticated_envelope","pre_approval_no_import",
     "exact_approval_operation","typed_terminal_and_host_ack","same_authority_after_reopen",
     "same_device_after_reopen","owned_shutdown"],
   network_delivery_asserted:false}' > "$scratch"
chmod 600 "$scratch"
ln "$scratch" "$output"
rm -f "$scratch"
printf 'paired preflight: %s\n' "$output"
