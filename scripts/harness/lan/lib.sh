# Source me: typed multi-host harness commands and authoritative observations.
# Raw keys/screens are frontend-conformance diagnostics only.
# shellcheck source=env.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
# The isolated shell fixture supplies a mock driver; normal LAN runs use the
# checkout's guarded owner driver.
AURA_E2E_DRV="${AURA_E2E_DRV:-$AURA_E2E_ROOT/scripts/harness/lan/drv.sh}"

req() { # req <instance> <method> [JSON object fields] [transport timeout seconds]
  local inst=$1 method=$2 extra=${3:-} transport_timeout=${4:-60} body remote_nix remote_command
  body=$(jq -cn --arg instance "$inst" --arg method "$method" --argjson extra "{${extra}}" \
    '{method:$method,params:({instance_id:$instance}+$extra)}') || return
  if [[ -n "${AURA_E2E_LOCAL_PEER_ROOT:-}" && "$inst" == "$AURA_E2E_REMOTE_PREFIX"* ]]; then
    timeout "$transport_timeout" env AURA_E2E_ROOT="$AURA_E2E_LOCAL_PEER_ROOT" \
      AURA_E2E_RUN_DIR="$AURA_E2E_LOCAL_PEER_RUN_DIR" \
      AURA_E2E_RUN_TOKEN="$AURA_E2E_RUN_TOKEN" \
      "$AURA_E2E_LOCAL_PEER_ROOT/scripts/harness/lan/drv.sh" req "$body" "$transport_timeout"
  elif [ -n "$AURA_E2E_REMOTE" ] && [[ "$inst" == "$AURA_E2E_REMOTE_PREFIX"* ]]; then
    remote_nix=${AURA_E2E_REMOTE_NIX:-/nix/var/nix/profiles/default/bin/nix}
    printf -v remote_command 'cd %q && %q develop --command scripts/harness/lan/drv.sh req %q %q' \
      "$AURA_E2E_REMOTE_ROOT" "$remote_nix" "$body" "$transport_timeout"
    timeout "$transport_timeout" ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
      "$remote_command"
  else
    timeout "$transport_timeout" "$AURA_E2E_DRV" req "$body" "$transport_timeout"
  fi
}
# Private enrollment requests enter the selected ToolApi owner through its
# socket. The complete JSON body is supplied on stdin; neither request nor
# response passes through process arguments or retained repl.out.
req_private() { # req_private <instance> [transport timeout seconds] < request.json
  local inst=$1 transport_timeout=${2:-60} remote_nix remote_command
  if [[ -n "${AURA_E2E_LOCAL_PEER_ROOT:-}" && "$inst" == "$AURA_E2E_REMOTE_PREFIX"* ]]; then
    jq -ce --arg instance "$inst" \
      'select(type=="object" and (.method|type=="string") and (.params|type=="object"))
       | .params.instance_id=$instance' \
      | timeout "$transport_timeout" env AURA_E2E_ROOT="$AURA_E2E_LOCAL_PEER_ROOT" \
        AURA_E2E_RUN_DIR="$AURA_E2E_LOCAL_PEER_RUN_DIR" \
        AURA_E2E_RUN_TOKEN="$AURA_E2E_RUN_TOKEN" \
        "$AURA_E2E_LOCAL_PEER_ROOT/scripts/harness/lan/drv.sh" req-private
  elif [ -n "$AURA_E2E_REMOTE" ] && [[ "$inst" == "$AURA_E2E_REMOTE_PREFIX"* ]]; then
    [[ "$AURA_E2E_RUN_TOKEN" =~ ^[a-z0-9][a-z0-9-]{15,127}$ ]] || {
      echo 'private LAN request requires the exact run token' >&2; return 2;
    }
    remote_nix=${AURA_E2E_REMOTE_NIX:-/nix/var/nix/profiles/default/bin/nix}
    printf -v remote_command 'cd %q && AURA_E2E_RUN_TOKEN=%q %q develop --command scripts/harness/lan/drv.sh req-private' \
      "$AURA_E2E_REMOTE_ROOT" "$AURA_E2E_RUN_TOKEN" "$remote_nix"
    jq -ce --arg instance "$inst" \
      'select(type=="object" and (.method|type=="string") and (.params|type=="object"))
       | .params.instance_id=$instance' \
      | timeout "$transport_timeout" ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
        "$remote_command"
  else
    jq -ce --arg instance "$inst" \
      'select(type=="object" and (.method|type=="string") and (.params|type=="object"))
       | .params.instance_id=$instance' \
      | timeout "$transport_timeout" "$AURA_E2E_DRV" req-private
  fi
}

# Identify the actual sourced helper separately from the fixed candidate's
# production revision. A private archived tooling bundle proves its committed
# source and all three files in the paired-script load closure.
lan_private_tooling_identity() {
  local tooling_dir bundle_root manifest commit name expected actual committed lib_hash
  tooling_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P) || return 2
  bundle_root=$(cd "$tooling_dir/../../.." && pwd -P) || return 2
  lib_hash=$(sha256sum "$tooling_dir/lib.sh" | cut -d' ' -f1) || return 2
  manifest="$bundle_root/provenance.json"
  if [[ ! -e "$manifest" && ! -L "$manifest" ]]; then
    jq -cn --arg hash "$lib_hash" \
      '{revision:null,lib_sha256:$hash,committed_bundle_verified:false}'
    return
  fi
  [[ -f "$manifest" && ! -L "$manifest" && -O "$manifest" &&
     -d "$bundle_root" && ! -L "$bundle_root" && -O "$bundle_root" ]] || return 2
  [[ $(stat -c '%a' "$manifest" 2>/dev/null || stat -f '%Lp' "$manifest") == 600 &&
     $(stat -c '%a' "$bundle_root" 2>/dev/null || stat -f '%Lp' "$bundle_root") == 700 ]] || return 2
  commit=$(jq -er '.tooling_commit|select(type=="string" and test("^[0-9a-f]{40}$"))' "$manifest") || return 2
  for name in paired-enrollment.sh lib.sh env.sh; do
    expected=$(jq -er --arg name "$name" '.files[$name]|select(type=="string" and test("^[0-9a-f]{64}$"))' "$manifest") || return 2
    actual=$(sha256sum "$tooling_dir/$name" | cut -d' ' -f1) || return 2
    committed=$(git -C "$AURA_E2E_ROOT" show "$commit:scripts/harness/lan/$name" | sha256sum | cut -d' ' -f1) || return 2
    [[ "$actual" == "$expected" && "$committed" == "$expected" ]] || return 2
  done
  jq -cn --arg revision "$commit" --arg hash "$lib_hash" \
    '{revision:$revision,lib_sha256:$hash,committed_bundle_verified:true}'
}

lan_private_remove_owned_link() { # linked path, this invocation's temporary inode
  local linked=$1 original=$2
  if [[ -n "$linked" && -n "$original" && -f "$linked" && ! -L "$linked" &&
        "$linked" -ef "$original" ]]; then
    rm -f "$linked"
  fi
  return 0
}

# Whitelist only nonsecret facts required for paired enrollment evidence. The
# caller passes a private response on stdin. Keep raw failures only in owner-only
# custody; never send them to shared evidence.
lan_private_receipt() { # instance method label [expected-operation] < response.json
  local instance=$1 method=$2 label=$3 expected_operation=${4:-} dir target scratch raw validator_stderr mode raw_target reason_target stderr_target reason
  local failure_target='' custody_target='' custody_tmp='' source_revision raw_hash tooling_identity
  case "$instance" in alex-tui|alex-tui2|alex-web|barbara-tui|barbara-tui2|barbara-web) ;; *) return 2;; esac
  case "$method" in prepare_device_enrollment_setup|submit_semantic_command|ui_state|wait_for_ui_snapshot_event|restart|get_authority_id) ;; *) return 2;; esac
  [[ "$label" =~ ^[a-z0-9][a-z0-9-]{0,63}$ ]] || return 2
  if [[ -n "$expected_operation" ]]; then
    [[ "$method" == wait_for_ui_snapshot_event ]] || return 2
    if [[ "$instance" == barbara-web ]]; then
      [[ "$expected_operation" =~ ^op-[0-9]{1,20}$ ]] || return 2
    else
      [[ "$expected_operation" =~ ^tui-op-device_enrollment-[0-9]{1,20}$ ]] || return 2
    fi
  fi
  [[ "$AURA_E2E_RUN_TOKEN" =~ ^[a-z0-9][a-z0-9-]{15,127}$ ]] || return 2
  [[ -d "$AURA_E2E_RUN_DIR" && ! -L "$AURA_E2E_RUN_DIR" && -O "$AURA_E2E_RUN_DIR" ]] || return 2
  dir="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN"
  if [[ ! -e "$dir" && ! -L "$dir" ]]; then mkdir -m 700 "$dir" || return; fi
  [[ -d "$dir" && ! -L "$dir" && -O "$dir" ]] || return 2
  mode=$(stat -c '%a' "$dir" 2>/dev/null || stat -f '%Lp' "$dir") || return
  [[ "$mode" == 700 ]] || return 2
  target="$dir/$instance-$label.json"
  [[ ! -e "$target" && ! -L "$target" ]] || return 2
  scratch=$(mktemp "$dir/.receipt.XXXXXX") || return
  raw=$(mktemp "$dir/.raw-response.XXXXXX") || { rm -f "$scratch"; return 2; }
  validator_stderr=$(mktemp "$dir/.validator-stderr.XXXXXX") || { rm -f "$scratch" "$raw"; return 2; }
  chmod 600 "$raw" "$validator_stderr" || { rm -f "$scratch" "$raw" "$validator_stderr"; return 2; }
  head -c 1048577 > "$raw" || { rm -f "$scratch" "$raw" "$validator_stderr"; return 2; }
  if (( $(wc -c < "$raw") > 1048576 )); then
    local prefix_target="$dir/$instance-$label.raw-prefix-private.bin"
    reason_target="$dir/$instance-$label.validation.json"
    [[ ! -e "$prefix_target" && ! -L "$prefix_target" &&
       ! -e "$reason_target" && ! -L "$reason_target" ]] || {
      rm -f "$scratch" "$raw" "$validator_stderr"; return 2;
    }
    head -c 1048576 "$raw" > "$scratch" || {
      rm -f "$scratch" "$raw" "$validator_stderr"; return 2;
    }
    chmod 600 "$scratch" || { rm -f "$scratch" "$raw" "$validator_stderr"; return 2; }
    ln "$scratch" "$prefix_target" || { rm -f "$scratch" "$raw" "$validator_stderr"; return 2; }
    local reason_tmp
    reason_tmp=$(mktemp "$dir/.validation.XXXXXX") || {
      rm -f "$scratch" "$raw" "$validator_stderr" "$prefix_target"; return 2;
    }
    chmod 600 "$reason_tmp" || {
      rm -f "$scratch" "$raw" "$validator_stderr" "$prefix_target" "$reason_tmp"; return 2;
    }
    jq -cn --arg token "$AURA_E2E_RUN_TOKEN" \
      --arg instance "$instance" --arg method "$method" --arg label "$label" \
      '{schema:1,run_token:$token,instance_id:$instance,method:$method,label:$label,
        validation_failure:"response_too_large",capture_limit_bytes:1048576,
        captured_prefix_bytes:1048576,truncated:true,
        raw_response_custody:"owner_only_bounded_prefix"}' > "$reason_tmp" || {
      rm -f "$scratch" "$raw" "$validator_stderr" "$prefix_target" "$reason_tmp"; return 2;
    }
    ln "$reason_tmp" "$reason_target" || {
      rm -f "$scratch" "$raw" "$validator_stderr" "$prefix_target" "$reason_tmp"; return 2;
    }
    rm -f "$scratch" "$raw" "$validator_stderr" "$reason_tmp"
    return 2
  fi
  if ! jq -ce --arg instance "$instance" --arg method "$method" '
    def op_id: type=="string" and
      (if $instance=="barbara-web" then test("^op-[0-9]{1,20}$")
       else test("^tui-op-device_enrollment-[0-9]{1,20}$") end);
    def failure:
      . == null or (type=="object"
        and (.domain|IN("command","ceremony","invitation","channel_context","transport","delivery","projection","internal"))
        and (.code|IN("unsupported_command","permission_denied","budget_exceeded","not_member","not_found","invalid_argument","invalid_state","muted","banned","unavailable","command_failed","crypto_failure","serialization_failure","storage_failure","journal_failure","reactive_failure","missing_authoritative_context","contact_link_did_not_converge","channel_bootstrap_unavailable","peer_channel_not_established","delivery_readiness_not_reached","operation_timed_out","ceremony_rejected","ceremony_cancelled","ceremony_choreography_failed","ceremony_runtime_failed","ceremony_superseded","shell_declared_success_illegally","invitation_revoked","invitation_expired","invitation_already_settled","inviter_did_not_confirm","internal_error")));
    def operation:
      .id=="device_enrollment" and (.instance_id|op_id)
      and (.state|IN("idle","submitting","succeeded","failed","cancelled"))
      and (.failure|failure);
    def handle: .id=="device_enrollment" and (.instance_id|op_id);
    def host_ack:
      ((if $instance=="barbara-web" then (.id|type=="string" and test("^runtime-event-[1-9][0-9]{0,19}$"))
         else (.id|type=="string" and startswith("tui-runtime-fact-")) end))
      and .fact.kind=="device_enrollment_accepted"
      and (.fact.device_id|type=="string" and test("^[0-9a-fA-F-]{36}$"))
      and (.fact.device_count|type=="number" and .>=1 and .<=16 and .==floor);
    def tool_error_code($message):
      if $message|contains("structured ui_state observation failed for instance")
      then "browser_structured_observation_failed"
      elif $message|contains("browser UI state unavailable for instance")
      then "browser_ui_state_unavailable"
      elif $message|startswith("Playwright driver ui_state timed out")
      then "browser_driver_timeout"
      elif $message|startswith("Playwright driver closed stdout while awaiting ui_state")
      then "browser_driver_closed"
      elif $message|startswith("failed to decode browser UiSnapshot")
      then "browser_snapshot_decode_failed"
      else "unclassified" end;
    . as $response
    | ($response.payload.event.snapshot // $response.payload) as $snapshot
    | [($snapshot.operations // [])[] | select(.id=="device_enrollment")] as $operations
    | [($snapshot.runtime_events // [])[] | select(.fact.kind=="device_enrollment_accepted")] as $acks
    | ($response.payload.handle.ui_operation // null) as $submitted
    | if ($response.status|IN("ok","error"))
      and ($response.status!="error" or ($response.message|type=="string"))
      and ([$operations[]|operation]|all)
      and ([$acks[]|host_ack]|all)
      and ($submitted==null or ($submitted|handle))
      and ($method!="restart" or ($response.payload.status|IN("restarted",null)))
      and ($snapshot.readiness==null or ($snapshot.readiness|IN("ready","loading")))
      then {
        instance_id:$instance, method:$method, status:$response.status,
        tool_failure:(if $response.status=="error" then
          {domain:"tool_api",code:tool_error_code($response.message)} else null end),
        submitted_operation:(if $submitted==null then null else {id:$submitted.id,instance_id:$submitted.instance_id} end),
        operations:[$operations[]|{id,instance_id,state,failure:(.failure // null|if .==null then null else {domain,code} end)}],
        host_acknowledgments:[$acks[]|{device_id:.fact.device_id,device_count:.fact.device_count}],
        ready:($snapshot.readiness=="ready"),
        restarted:($method=="restart" and $response.payload.status=="restarted")
      } else empty end
  ' "$raw" > "$scratch" 2> "$validator_stderr" || [[ ! -s "$scratch" ]]; then
    rm -f "$scratch"
    raw_target="$dir/$instance-$label.raw-private.json"
    reason_target="$dir/$instance-$label.validation.json"
    stderr_target="$dir/$instance-$label.validator-private.txt"
    [[ ! -e "$raw_target" && ! -L "$raw_target" &&
       ! -e "$reason_target" && ! -L "$reason_target" &&
       ! -e "$stderr_target" && ! -L "$stderr_target" ]] || { rm -f "$raw" "$validator_stderr"; return 2; }
    reason=$(jq -er --arg instance "$instance" '
      if (.status|IN("ok","error")|not) then "invalid_status"
      elif .status=="error" and (.message|type)!="string" then "invalid_tool_error"
      elif any((.payload.event.snapshot // .payload).runtime_events[]?;
        .fact.kind=="device_enrollment_accepted" and
        (if $instance=="barbara-web" then
          (if (.id|type)=="string" then (.id|test("^runtime-event-[1-9][0-9]{0,19}$")|not) else true end)
         else (if (.id|type)=="string" then (.id|startswith("tui-runtime-fact-")|not) else true end) end))
      then "invalid_host_ack_id"
      else "unsupported_receipt_shape" end
    ' "$raw" 2>/dev/null) || reason=malformed_json
    (umask 077; set -C; jq -cn --arg instance "$instance" --arg method "$method" \
      --arg reason "$reason" \
      '{schema:1,instance_id:$instance,method:$method,validation_failure:$reason,
        raw_response_custody:"owner_only_private_artifact"}' > "$reason_target") || { rm -f "$raw" "$validator_stderr"; return 2; }
    chmod 600 "$reason_target" || { rm -f "$raw" "$validator_stderr"; return 2; }
    ln "$raw" "$raw_target" || { rm -f "$raw" "$validator_stderr"; return 2; }
    if [[ -s "$validator_stderr" ]]; then
      ln "$validator_stderr" "$stderr_target" || { rm -f "$raw" "$validator_stderr"; return 2; }
    fi
    rm -f "$raw" "$validator_stderr"
    return 2
  fi
  [[ -s "$scratch" ]] || { rm -f "$scratch" "$raw" "$validator_stderr"; return 2; }
  chmod 600 "$scratch" || { rm -f "$scratch" "$raw" "$validator_stderr"; return 2; }
  if [[ -n "$expected_operation" ]]; then
    # An unrelated failed enrollment must not be recorded as the original one.
    if jq -e --arg operation "$expected_operation" '
      any((.payload.event.snapshot // .payload).operations[]?;
        .id=="device_enrollment" and (.state=="failed" or .state=="cancelled")
        and .instance_id!=$operation)' "$raw" >/dev/null 2>&1; then
      rm -f "$scratch" "$raw" "$validator_stderr"; return 2
    fi
    if jq -e --arg operation "$expected_operation" '
      any((.payload.event.snapshot // .payload).operations[]?;
        .id=="device_enrollment" and .instance_id==$operation and
        (.state=="failed" or .state=="cancelled"))' "$raw" >/dev/null 2>&1; then
      failure_target="$dir/$instance-$label.failure-raw-private.json"
      custody_target="$dir/$instance-$label.failure-custody.json"
      [[ ! -e "$failure_target" && ! -L "$failure_target" &&
         ! -e "$custody_target" && ! -L "$custody_target" ]] || {
        rm -f "$scratch" "$raw" "$validator_stderr"; return 2;
      }
      source_revision=$(git -C "$AURA_E2E_ROOT" rev-parse HEAD) || {
        rm -f "$scratch" "$raw" "$validator_stderr"; return 2;
      }
      [[ "$source_revision" =~ ^[0-9a-f]{40}$ ]] || {
        rm -f "$scratch" "$raw" "$validator_stderr"; return 2;
      }
      raw_hash=$(sha256sum "$raw" | cut -d' ' -f1) || {
        rm -f "$scratch" "$raw" "$validator_stderr"; return 2;
      }
      tooling_identity=$(lan_private_tooling_identity) || {
        rm -f "$scratch" "$raw" "$validator_stderr"; return 2;
      }
      custody_tmp=$(mktemp "$dir/.failure-custody.XXXXXX") || {
        rm -f "$scratch" "$raw" "$validator_stderr"; return 2;
      }
      chmod 600 "$custody_tmp" || {
        rm -f "$scratch" "$raw" "$validator_stderr" "$custody_tmp"; return 2;
      }
      jq -cn --arg source "$source_revision" --arg token "$AURA_E2E_RUN_TOKEN" \
        --arg instance "$instance" --arg method "$method" --arg label "$label" \
        --arg operation "$expected_operation" --arg hash "$raw_hash" \
        --arg raw_name "${failure_target##*/}" --arg redacted_name "${target##*/}" \
        --argjson tooling "$tooling_identity" \
        '{schema:1,source_revision:$source,run_token:$token,instance_id:$instance,
          method:$method,label:$label,operation_instance:$operation,
          raw_response_sha256:$hash,raw_response_file:$raw_name,
          redacted_receipt_file:$redacted_name,tooling:$tooling,
          raw_response_custody:"owner_only_private_artifact"}' \
        > "$custody_tmp" || {
          rm -f "$scratch" "$raw" "$validator_stderr" "$custody_tmp"; return 2;
        }
      ln "$raw" "$failure_target" || {
        rm -f "$scratch" "$raw" "$validator_stderr" "$custody_tmp"; return 2;
      }
      ln "$custody_tmp" "$custody_target" || {
        lan_private_remove_owned_link "$failure_target" "$raw"
        rm -f "$scratch" "$raw" "$validator_stderr" "$custody_tmp"; return 2;
      }
    fi
  fi
  ln "$scratch" "$target" || {
    lan_private_remove_owned_link "$failure_target" "$raw"
    lan_private_remove_owned_link "$custody_target" "$custody_tmp"
    rm -f "$scratch" "$raw" "$validator_stderr"
    [[ -z "$custody_tmp" ]] || rm -f "$custody_tmp"
    return 2
  }
  rm -f "$scratch" "$raw" "$validator_stderr"
  [[ -z "$custody_tmp" ]] || rm -f "$custody_tmp"
  printf '%s\n' "$target"
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
