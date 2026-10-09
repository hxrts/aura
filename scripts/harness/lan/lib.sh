# Source me: typed multi-host harness commands and authoritative observations.
# Raw keys/screens are frontend-conformance diagnostics only.
# shellcheck source=env.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
AURA_E2E_DRV="$AURA_E2E_ROOT/scripts/harness/lan/drv.sh"

req() { # req <instance> <method> [JSON object fields] [transport timeout seconds]
  local inst=$1 method=$2 extra=${3:-} transport_timeout=${4:-60} body quoted_body quoted_root
  body=$(jq -cn --arg instance "$inst" --arg method "$method" --argjson extra "{${extra}}" \
    '{method:$method,params:({instance_id:$instance}+$extra)}') || return
  if [ -n "$AURA_E2E_REMOTE" ] && [[ "$inst" == "$AURA_E2E_REMOTE_PREFIX"* ]]; then
    quoted_body=${body//\'/\'\\\'\'}
    quoted_root=${AURA_E2E_REMOTE_ROOT//\'/\'\\\'\'}
    timeout "$transport_timeout" ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
      "cd '$quoted_root' && scripts/harness/lan/drv.sh req '$quoted_body' $transport_timeout"
  else
    timeout "$transport_timeout" "$AURA_E2E_DRV" req "$body" "$transport_timeout"
  fi
}
key() { req "$1" send_key "$(jq -cn --arg key "$2" '{key:$key}' | sed 's/^{//;s/}$//')" >/dev/null; }
keys() { req "$1" send_keys "$(jq -cn --arg keys "$2" '{keys:$keys}' | sed 's/^{//;s/}$//')" 120 >/dev/null; }
st() { req "$1" ui_state | jq -ce ".payload|${2:-.}"; }
scr() { req "$1" screen | jq -r .payload.diagnostic_authoritative_screen | sed -n "${2:-1,200p}"; }
chans() { req "$1" diagnostic_list_channels | jq -c '[.payload.diagnostic_channels[]|(if .selected then "*" else "" end)+.name]'; }
contacts() { st "$1" '[.lists[]|select(.id=="contacts")|.items[].id]'; }
ops() { st "$1" '[.operations[]|{id,instance_id,state,failure}]'; }
toasts() { st "$1" '[.toasts[]|.message]'; }
deadtasks() { st "$1" '(.supervised_task_failures // [])[]|"\(.group)/\(.task): \(.cause)"' | jq -r .; }
drops() { st "$1" '(.message_drops // [])[]|"\(.direction) \(.peer_id // "-") \(.channel_id // "-") msg=\(.message_id // "-"): \(.reason)"' | jq -r .; }

# SECONDS is the local Bash elapsed-time clock. Every composed action passes
# its original deadline through all command and pushed-event waits; receiving
# an unrelated event never resets that observation budget.
lan_deadline() { echo "$((SECONDS + ${1:-${AURA_E2E_SEMANTIC_TIMEOUT_SECONDS:-60}}))"; }
lan_remaining() {
  local remaining=$(($1 - SECONDS))
  if ((remaining <= 0)); then echo 'authoritative LAN observation deadline elapsed' >&2; return 1; fi
  echo "$remaining"
}
lan_evidence() {
  mkdir -p "$AURA_E2E_RUN_DIR"
  jq -cn --arg instance "$1" --arg method "$2" --argjson response "$3" \
    '{instance_id:$instance,method:$method,response:$response}' >> "$AURA_E2E_RUN_DIR/semantic-evidence-$AURA_E2E_RUN_TOKEN.jsonl"
}
wait_snapshot() { # instance, jq predicate, predicate variables object, original deadline
  local inst=$1 predicate=$2 variables=${3:-'{}'} deadline=${4:-$(lan_deadline)}
  local version=null remaining response event snapshot
  while remaining=$(lan_remaining "$deadline"); do
    response=$(req "$inst" wait_for_ui_snapshot_event \
      "\"timeout_ms\":$((remaining * 1000)),\"after_version\":$version" "$remaining") || return
    lan_evidence "$inst" wait_for_ui_snapshot_event "$response" || return
    lan_remaining "$deadline" >/dev/null || return
    if ! jq -e '.status == "ok" and .payload.event != null' <<< "$response" >/dev/null; then
      echo "authoritative snapshot wait failed for $inst: $response" >&2; return 1
    fi
    event=$(jq -c '.payload.event' <<< "$response") || return
    version=$(jq -er '.version' <<< "$event") || return
    snapshot=$(jq -c '.snapshot' <<< "$event") || return
    if jq -e --argjson vars "$variables" "$predicate" <<< "$snapshot" >/dev/null; then
      echo "$snapshot"; return 0
    fi
  done
  return 1
}
wait_operation() { # instance, exact returned ui_operation handle, original deadline
  local inst=$1 handle=$2 deadline=$3 snapshot state
  snapshot=$(wait_snapshot "$inst" \
    'any(.operations[]; .id == $vars.id and .instance_id == $vars.instance_id and (.state == "succeeded" or .state == "failed" or .state == "cancelled"))' \
    "$handle" "$deadline") || return
  state=$(jq -cr --argjson handle "$handle" \
    '.operations[]|select(.id==$handle.id and .instance_id==$handle.instance_id)' <<< "$snapshot") || return
  if ! jq -e '.state == "succeeded"' <<< "$state" >/dev/null; then
    echo "semantic operation failed for $inst: $state" >&2; return 1
  fi
}
semantic() { # instance, typed IntentAction JSON, original deadline (optional)
  local inst=$1 intent=$2 deadline=${3:-$(lan_deadline)} remaining response handle required_operation
  remaining=$(lan_remaining "$deadline") || return
  response=$(req "$inst" submit_semantic_command "\"intent\":$intent" "$remaining") || return
  lan_evidence "$inst" submit_semantic_command "$response" || return
  lan_remaining "$deadline" >/dev/null || return
  if ! jq -e '.status == "ok" and .payload.submission == "accepted"' <<< "$response" >/dev/null; then
    echo "semantic submission failed for $inst: $response" >&2; return 1
  fi
  handle=$(jq -c '.payload.handle.ui_operation' <<< "$response") || return
  required_operation=$(jq -c '.payload.contract.submission|if .kind=="operation_handle" then .operation_id else null end' <<< "$response") || return
  if jq -e '.payload.contract.submission.kind=="operation_handle"' <<< "$response" >/dev/null; then
    [[ "$required_operation" != null ]] || { echo 'canonical operation id is missing' >&2; return 1; }
    jq -e --argjson required "$required_operation" '.payload.handle.ui_operation.id == $required' <<< "$response" >/dev/null || {
      echo "semantic receipt is missing its canonical operation handle: $response" >&2; return 1;
    }
    wait_operation "$inst" "$handle" "$deadline" || return
  elif ! jq -e '.payload.contract.submission.kind=="immediate"' <<< "$response" >/dev/null; then
    echo "semantic receipt has no canonical submission contract: $response" >&2; return 1
  fi
  jq -c '.payload' <<< "$response"
}
onboard() { # instance, account name: shared TUI/web contract
  local deadline response snapshot authority_id
  deadline=$(lan_deadline)
  wait_snapshot "$1" '.screen == "onboarding"' '{}' "$deadline" >/dev/null || return
  semantic "$1" "$(jq -cn --arg name "$2" '{CreateAccount:{account_name:$name}}')" "$deadline" >/dev/null || return
  wait_snapshot "$1" '.readiness == "ready" and .screen == "neighborhood"' '{}' "$deadline" >/dev/null || return
  snapshot=$(wait_snapshot "$1" '.screen=="neighborhood" and .readiness=="ready" and
    ([.lists[]?|select(.id=="authorities")|.items[]?|
      select(.selected and .confirmation=="confirmed" and (.id|type=="string") and (.id|length>0))]|length)==1' '{}' "$deadline") || return
  authority_id=$(jq -er '[.lists[]|select(.id=="authorities")|.items[]|select(.selected and .confirmation=="confirmed")][0].id' <<< "$snapshot") || return
  response=$(req "$1" get_authority_id '' "$(lan_remaining "$deadline")") || return
  lan_evidence "$1" get_authority_id "$response" || return
  lan_remaining "$deadline" >/dev/null || return
  jq -e --arg authority "$authority_id" '.status=="ok" and .payload.authority_id==$authority' <<< "$response" >/dev/null || {
    echo "runtime authority does not match the selected canonical authority for $1: $response" >&2; return 1;
  }
  printf '%s\n' "$authority_id"
}
open_screen() { # instance, shared screen id, original deadline
  local inst=$1 screen=$2 deadline=${3:-$(lan_deadline)}
  semantic "$inst" "$(jq -cn --arg screen "$screen" '{OpenScreen:{screen:$screen,channel_id:null,context_id:null}}')" "$deadline" >/dev/null || return
  wait_snapshot "$inst" '.screen==$vars.screen and .readiness=="ready" and .quiescence.state=="settled"' \
    "$(jq -cn --arg screen "$screen" '{screen:$screen}')" "$deadline" >/dev/null
}
link() { # inviter, accepter: exact returned invitation code and operation handles
  local inviter=$1 accepter=$2 deadline inviter_id accepter_id receipt code
  deadline=$(lan_deadline)
  open_screen "$inviter" contacts "$deadline" || return
  open_screen "$accepter" contacts "$deadline" || return
  inviter_id=$(req "$inviter" get_authority_id '' "$(lan_remaining "$deadline")" | jq -er '.payload.authority_id') || return
  accepter_id=$(req "$accepter" get_authority_id '' "$(lan_remaining "$deadline")" | jq -er '.payload.authority_id') || return
  receipt=$(semantic "$inviter" "$(jq -cn --arg authority "$accepter_id" \
    '{CreateContactInvitation:{receiver_authority_id:$authority,code_name:null}}')" "$deadline") || return
  code=$(jq -er '.value|select(.kind=="contact_invitation_code")|.code' <<< "$receipt") || return
  semantic "$accepter" "$(jq -cn --arg code "$code" '{AcceptContactInvitation:{code:$code}}')" "$deadline" >/dev/null || return
  wait_snapshot "$inviter" 'any(.lists[]; .id=="contacts" and any(.items[]; .id==$vars.authority))' \
    "$(jq -cn --arg authority "$accepter_id" '{authority:$authority}')" "$deadline" >/dev/null || return
  wait_snapshot "$accepter" 'any(.lists[]; .id=="contacts" and any(.items[]; .id==$vars.authority))' \
    "$(jq -cn --arg authority "$inviter_id" '{authority:$authority}')" "$deadline" >/dev/null || return
  echo "linked $inviter -> $accepter"
}
restart_ok() {
  local deadline=$(lan_deadline) response
  response=$(req "$1" restart '' "$(lan_remaining "$deadline")") || return
  jq -e '.status=="ok"' <<< "$response" >/dev/null || return
  wait_snapshot "$1" '.readiness == "ready"' '{}' "$deadline" >/dev/null || return
  response=$(req "$1" get_authority_id '' "$(lan_remaining "$deadline")") || return
  jq -er 'select(.status=="ok")|.payload.authority_id' <<< "$response"
}
rlog() { ls -t "$AURA_E2E_ROOT"/.tmp/harness/transient/*/"$1"/runtime.log 2>/dev/null | head -1; }
