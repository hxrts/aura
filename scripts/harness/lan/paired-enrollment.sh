#!/usr/bin/env bash
# Run after batch.sh's paired bootstrap. Secret-bearing ToolApi traffic stays
# on the selected owner's private socket and is never printed or retained raw.
set -euo pipefail
set +x
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
source "$here/lib.sh"

[[ "${AURA_E2E_BOOTSTRAP:-}" == paired-enrollment ]] || {
  echo 'paired enrollment requires the unlinked paired bootstrap' >&2; exit 2;
}
[[ -n "$AURA_E2E_REMOTE" ]] || { echo 'paired enrollment requires the other host' >&2; exit 2; }
preflight="$AURA_E2E_ROOT/.tmp/lan-checklist-$AURA_E2E_RUN_TOKEN/paired-preflight.json"
commit=$(git -C "$AURA_E2E_ROOT" rev-parse HEAD)
jq -e --arg commit "$commit" --arg token "$AURA_E2E_RUN_TOKEN" '
  .phase=="preflight" and .commit.local==$commit and .commit.remote==$commit and
  .run_token==$token and .network_delivery_asserted==false' "$preflight" >/dev/null || {
  echo 'paired enrollment requires its fixed-commit preflight manifest' >&2; exit 1;
}

private_exchange() { # instance method receipt-label < single request JSON
  local instance=$1 method=$2 label=$3 transport_timeout=90
  [[ "$method" != wait_for_ui_snapshot_event ]] || transport_timeout=300
  PRIVATE_RESPONSE=$(req_private "$instance" "$transport_timeout") || {
    echo "private $method transport failed for $instance" >&2; return 1;
  }
  PRIVATE_RECEIPT=$(printf '%s\n' "$PRIVATE_RESPONSE" |
    lan_private_receipt "$instance" "$method" "$label") || {
    echo "private $method receipt validation failed for $instance" >&2; return 1;
  }
  if ! jq -e '.status=="ok"' <<< "$PRIVATE_RESPONSE" >/dev/null; then
    echo "private $method returned a typed refusal for $instance; inspect $PRIVATE_RECEIPT" >&2
    return 1
  fi
}

private_wait() { # instance label predicate jq program, expected operation instance
  local instance=$1 label=$2 predicate=$3 expected=$4 after=null deadline remaining response version request counter=0
  deadline=$((SECONDS + 240))
  while (( SECONDS < deadline )); do
    remaining=$((deadline - SECONDS))
    (( remaining > 0 )) || break
    counter=$((counter + 1))
    request=$(jq -cn --argjson after "$after" --argjson timeout "$((remaining * 1000))" \
      '{method:"wait_for_ui_snapshot_event",params:{after_version:$after,timeout_ms:$timeout}}') || return
    private_exchange "$instance" wait_for_ui_snapshot_event "$label-$counter" <<< "$request" || return
    response=$PRIVATE_RESPONSE
    jq -e '.payload.event.snapshot != null and (.payload.event.version|type=="number")' \
      <<< "$response" >/dev/null || return 1
    if jq -e --arg operation "$expected" '
      any(.payload.event.snapshot.operations[]?;
        .id=="device_enrollment" and .instance_id==$operation and
        (.state=="failed" or .state=="cancelled"))' <<< "$response" >/dev/null; then
      echo "device enrollment ended in typed refusal; inspect $PRIVATE_RECEIPT" >&2
      return 1
    fi
    if [[ "$label" == accepted ]] && jq -e --arg operation "$expected" '
      any(.payload.event.snapshot.operations[]?;
        .id=="device_enrollment" and .instance_id==$operation and .state=="succeeded") and
      ([.payload.event.snapshot.runtime_events[]?|
        select(.fact.kind=="device_enrollment_accepted" and .fact.device_count==2)]|length)==0' \
      <<< "$response" >/dev/null; then
      echo "device enrollment succeeded without its host acknowledgment; inspect $PRIVATE_RECEIPT" >&2
      return 1
    fi
    if jq -e --arg operation "$expected" "$predicate" <<< "$response" >/dev/null; then
      PRIVATE_RESPONSE=$response
      return 0
    fi
    version=$(jq -er '.payload.event.version' <<< "$response") || return
    after=$version
  done
  echo "authoritative enrollment observation timed out for $instance" >&2
  return 1
}

private_request() { # instance method label, static request JSON
  local instance=$1 method=$2 label=$3 request=$4
  private_exchange "$instance" "$method" "$label" <<< "$request"
}

private_request barbara-tui prepare_device_enrollment_setup setup \
  '{"method":"prepare_device_enrollment_setup","params":{}}'
setup_response=$PRIVATE_RESPONSE
jq -e '.payload.setup_code|type=="string" and length>0' <<< "$setup_response" >/dev/null || {
  echo 'signed setup export omitted its code' >&2; exit 1;
}

private_request alex-tui submit_semantic_command open-settings \
  '{"method":"submit_semantic_command","params":{"intent":{"OpenScreen":{"screen":"settings","channel_id":null,"context_id":null}}}}'
private_wait alex-tui settings-ready \
  '.payload.event.snapshot | .screen=="settings" and .readiness=="ready" and .quiescence.state=="settled"' ''

issue_request=$(jq -ce '{method:"submit_semantic_command",params:{intent:{StartDeviceEnrollment:{device_name:"Barbara paired device",code_name:"paired-enrollment",setup_code:.payload.setup_code}}}}' \
  <<< "$setup_response") || exit 1
private_request alex-tui submit_semantic_command issue "$issue_request"
issue_response=$PRIVATE_RESPONSE
issue_operation=$(jq -er '.payload.handle.ui_operation | select(.id=="device_enrollment") | .instance_id' <<< "$issue_response") || exit 1
[[ "$issue_operation" =~ ^tui-op-device_enrollment-[0-9]{1,20}$ ]] || exit 1

private_wait alex-tui issued '
  .payload.event.snapshot as $s |
  any($s.operations[]?; .id=="device_enrollment" and .instance_id==$operation and .state=="succeeded") and
  any($s.runtime_events[]?; .fact.kind=="device_enrollment_code_ready" and
    (.fact.code|type=="string" and length>0) and
    (.fact.manifest_transfer.manifest_code|type=="string" and length>0) and
    (.fact.manifest_transfer.initiator_verifier_code|type=="string" and length>0))' "$issue_operation"
issued_event=$PRIVATE_RESPONSE

# A first delivery must not silently approve the new authority on Barbara.
private_request barbara-tui ui_state before-approval '{"method":"ui_state","params":{}}'
jq -e '[.payload.operations[]?|select(.id=="device_enrollment")]|length==0' \
  <<< "$PRIVATE_RESPONSE" >/dev/null || {
  echo 'recipient already has a device enrollment operation before approval' >&2; exit 1;
}

import_request=$(jq -ce '
  [.payload.event.snapshot.runtime_events[]?|select(.fact.kind=="device_enrollment_code_ready" and
    .fact.code!=null and .fact.manifest_transfer!=null)] | last | .fact |
  {method:"submit_semantic_command",params:{intent:{ImportDeviceEnrollmentCode:{
    code:.code,manifest_transfer:{manifest_code:.manifest_transfer.manifest_code,
    initiator_verifier_code:.manifest_transfer.initiator_verifier_code}}}}}' <<< "$issued_event") || exit 1
private_request barbara-tui submit_semantic_command approve "$import_request"
unset setup_response issue_request issued_event import_request
import_operation=$(jq -er '.payload.handle.ui_operation | select(.id=="device_enrollment") | .instance_id' \
  <<< "$PRIVATE_RESPONSE") || exit 1
[[ "$import_operation" =~ ^tui-op-device_enrollment-[0-9]{1,20}$ ]] || exit 1

private_wait barbara-tui accepted '
  .payload.event.snapshot as $s |
  ([ $s.runtime_events[]?|select(.fact.kind=="device_enrollment_accepted" and .fact.device_count==2 and .fact.device_id!=null) ] | last) as $ack |
  if any($s.operations[]?; .id=="device_enrollment" and .instance_id==$operation and .state=="succeeded")
  then ($ack!=null and ([ $s.lists[]?|select(.id=="devices")|.items[]?|select(.id==$ack.fact.device_id) ]|length)==1)
  else false end' "$import_operation"
accepted_event=$PRIVATE_RESPONSE
accepted_receipt=$PRIVATE_RECEIPT

private_request alex-tui get_authority_id issuer-authority '{"method":"get_authority_id","params":{}}'
issuer_authority=$PRIVATE_RESPONSE
private_request barbara-tui get_authority_id accepted-authority '{"method":"get_authority_id","params":{}}'
accepted_authority=$PRIVATE_RESPONSE
printf '%s\n%s\n' "$issuer_authority" "$accepted_authority" |
  jq -es '.[0].payload.authority_id==.[1].payload.authority_id and
    (.[0].payload.authority_id|type=="string" and length>0)' >/dev/null || {
  echo 'adopted authority differs from issuer' >&2; exit 1;
}

restart_request='{"method":"restart","params":{"expected_operation_instance":null}}'
private_request barbara-tui restart restart "$restart_request"
jq -e '.payload.status=="restarted"' <<< "$PRIVATE_RESPONSE" >/dev/null || exit 1
private_wait barbara-tui reopened '
  .payload.event.snapshot as $s |
  $s.readiness=="ready" and
  ([ $s.lists[]?|select(.id=="devices")|.items[]? ]|length)==2' "$import_operation"
reopened_event=$PRIVATE_RESPONSE
reopened_receipt=$PRIVATE_RECEIPT
private_request barbara-tui get_authority_id reopened-authority '{"method":"get_authority_id","params":{}}'
reopened_authority=$PRIVATE_RESPONSE
printf '%s\n%s\n' "$accepted_authority" "$reopened_authority" |
  jq -es '.[0].payload.authority_id==.[1].payload.authority_id' >/dev/null || {
  echo 'reopened authority differs from accepted authority' >&2; exit 1;
}
printf '%s\n%s\n' "$accepted_event" "$reopened_event" | jq -es '
  (.[0].payload.event.snapshot.runtime_events |
    [ .[]|select(.fact.kind=="device_enrollment_accepted" and .fact.device_count==2) ]|last|.fact.device_id) as $device |
  [.[1].payload.event.snapshot.lists[]?|select(.id=="devices")|.items[]?|select(.id==$device)]|length==1' >/dev/null || {
  echo 'original accepted device is absent after reopen' >&2; exit 1;
}

quoted_root=${AURA_E2E_REMOTE_ROOT//\'/\'\\\'\'}
local_identity=$(jq -ce '{pid,birth,run_token}' "$AURA_E2E_RUN_DIR/repl.identity.json")
remote_identity=$(ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
  "cd '$quoted_root' && jq -ce '{pid,birth,run_token}' .tmp/e2e/run/repl.identity.json")
printf '%s\n%s\n' "$local_identity" "$remote_identity" |
  jq -es --arg token "$AURA_E2E_RUN_TOKEN" 'all(.[]; .run_token==$token and (.pid|type=="number") and (.birth|type=="string" and length>0))' >/dev/null || {
  echo 'paired host process identities differ from the run token' >&2; exit 1;
}
manifest_dir="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN"
manifest="$manifest_dir/paired-result.json"
[[ ! -e "$manifest" && ! -L "$manifest" ]] || exit 1
manifest_tmp=$(mktemp "$manifest_dir/.manifest.XXXXXX")
jq -cn --arg commit "$commit" --arg token "$AURA_E2E_RUN_TOKEN" \
  --arg preflight "$preflight" \
  --argjson local_identity "$local_identity" --argjson remote_identity "$remote_identity" \
  --arg issued "$issue_operation" --arg accepted "$import_operation" \
  --arg accepted_receipt "$accepted_receipt" --arg reopened_receipt "$reopened_receipt" '
  {schema:1,phase:"result",commit:$commit,run_token:$token,preflight_manifest:$preflight,
   process_identity:{local:$local_identity,remote:$remote_identity},
   inventory:["signed_setup_export","issue_code_manifest_verifier","pre_approval_no_import",
     "exact_approval_operation","host_ack_two_devices","same_authority_after_reopen",
     "same_device_after_reopen"],
   actors:{issuer:"alex-tui",recipient:"barbara-tui"},
   operation_instances:{issue:$issued,approval:$accepted},
   evidence:{accepted:$accepted_receipt,reopened:$reopened_receipt},
   assertions:{independent_accounts_before_issue:true,explicit_approval:true,
     exact_operation_and_host_ack:true,same_authority_after_reopen:true,
     same_device_after_reopen:true,two_devices_after_reopen:true},
   transport:{broker_policy:"development_only_lan_http",network_delivery_asserted:false}}' > "$manifest_tmp"
chmod 600 "$manifest_tmp"
ln "$manifest_tmp" "$manifest"
rm -f "$manifest_tmp"
printf 'paired enrollment evidence: %s\n' "$manifest"
