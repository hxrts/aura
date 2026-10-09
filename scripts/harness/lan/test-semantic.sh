#!/usr/bin/env bash
# Deterministic transport fixtures for LAN semantic sequencing; no live runtime.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
fixture_root=$(mktemp -d)
trap 'rm -rf "$fixture_root"' EXIT
export AURA_E2E_HOST_ADDR=127.0.0.1 AURA_E2E_REMOTE= AURA_E2E_RUN_TOKEN=semantic-driver-test
export AURA_E2E_RUN_DIR="$fixture_root/evidence"
# shellcheck source=lib.sh
. "$here/lib.sh"
fixture_index="$fixture_root/index"
fixture_responses="$fixture_root/responses"
fixture_requests="$fixture_root/requests"
# Pin fixture deadlines instead of inheriting caller runtime timeout policy.
lan_deadline() { echo 4242; }
lan_remaining() { [[ "$1" == 4242 ]] || { echo "deadline reset: $1" >&2; return 1; }; echo 7; }
req() {
  local index response
  index=$(cat "$fixture_index")
  jq -cn --arg method "$2" --argjson params "{${3:-}}" '{method:$method,params:$params}' >> "$fixture_requests"
  response=$(sed -n "$((index+1))p" "$fixture_responses")
  [[ -n "$response" ]] || { echo 'fixture exhausted' >&2; return 1; }
  echo "$((index+1))" > "$fixture_index"
  echo "$response"
}
reset_fixture() { echo 0 > "$fixture_index"; : > "$fixture_requests"; }
receipt=${AURA_LAN_FIXTURE_RECEIPT:-'{"status":"ok","payload":{"submission":"accepted","handle":{"ui_operation":{"id":"invitation_accept_contact","instance_id":"original-7"}},"value":{"kind":"none"},"contract":{"submission":{"kind":"operation_handle","operation_id":"invitation_accept_contact","value":"none"}}}}'}
operation_id=$(jq -er '.payload.handle.ui_operation.id' <<< "$receipt")
intent='{"AcceptContactInvitation":{"code":"original-code"}}'
event() {
  jq -cn --argjson version "$1" --arg instance "$2" --arg state "$3" --arg operation "$operation_id" \
    '{status:"ok",payload:{event:{version:$version,snapshot:{operations:[{id:$operation,instance_id:$instance,state:$state}]}}}}'
}
reset_fixture
{
  echo "$receipt"
  event 11 old-operation succeeded
  event 12 original-7 submitting
  event 13 original-7 succeeded
} > "$fixture_responses"
result=$(semantic alice "$intent" 4242)
jq -e '.handle.ui_operation.instance_id=="original-7"' <<< "$result" >/dev/null
jq -se '.[0].method=="submit_semantic_command" and .[0].params.intent.AcceptContactInvitation.code=="original-code" and
  (.[1:]|map(.params.after_version)) == [null,11,12] and all(.[1:][]; .params.timeout_ms==7000)' "$fixture_requests" >/dev/null

reset_fixture
{ echo "$receipt"; event 14 original-7 failed; } > "$fixture_responses"
if semantic alice "$intent" 4242 > "$fixture_root/out" 2> "$fixture_root/error"; then
  echo 'failed operation incorrectly succeeded' >&2; exit 1
fi
grep -q 'semantic operation failed' "$fixture_root/error"

reset_fixture
printf '%s\n' "$receipt" '{"status":"ok","payload":{"event":null}}' > "$fixture_responses"
if semantic alice "$intent" 4242 > "$fixture_root/out" 2> "$fixture_root/error"; then
  echo 'absent observation incorrectly succeeded' >&2; exit 1
fi
grep -q 'authoritative snapshot wait failed' "$fixture_root/error"

reset_fixture
printf '%s\n' '{"status":"error","message":"command rejected"}' > "$fixture_responses"
if semantic alice "$intent" 4242 > "$fixture_root/out" 2> "$fixture_root/error"; then
  echo 'rejected command incorrectly succeeded' >&2; exit 1
fi
[[ $(cat "$fixture_index") == 1 ]]
jq -se 'any(.[]; .method=="submit_semantic_command") and any(.[]; .method=="wait_for_ui_snapshot_event" and .response.payload.event.version==13)' \
  "$AURA_E2E_RUN_DIR/semantic-evidence-$AURA_E2E_RUN_TOKEN.jsonl" >/dev/null
# Immediate account creation may return a pre-reload operation handle. The
# canonical contract requires the new shell's Neighborhood+Ready observation.
reset_fixture
{
  echo '{"status":"ok","payload":{"event":{"version":3,"snapshot":{"screen":"onboarding","readiness":"loading"}}}}'
  if [[ -n "${AURA_LAN_FIXTURE_IMMEDIATE_RECEIPT:-}" ]]; then
    echo "$AURA_LAN_FIXTURE_IMMEDIATE_RECEIPT"
  else
    jq -c '.payload.contract.submission={kind:"immediate",value:"none"}' <<< "$receipt"
  fi
  echo '{"status":"ok","payload":{"event":{"version":4,"snapshot":{"screen":"contacts","readiness":"ready"}}}}'
  echo '{"status":"ok","payload":{"event":{"version":5,"snapshot":{"screen":"neighborhood","readiness":"ready"}}}}'
  echo '{"status":"ok","payload":{"authority_id":"runtime-owned-authority"}}'
} > "$fixture_responses"
[[ $(onboard alice Alice) == runtime-owned-authority ]]
[[ $(cat "$fixture_index") == 5 ]]
jq -se '[.[]|select(.method=="wait_for_ui_snapshot_event")|.params.after_version] == [null,null,4]' "$fixture_requests" >/dev/null

reset_fixture
jq -c '.payload.handle.ui_operation=null' <<< "$receipt" > "$fixture_responses"
if semantic alice "$intent" 4242 > "$fixture_root/out" 2> "$fixture_root/error"; then
  echo 'missing canonical operation handle incorrectly succeeded' >&2; exit 1
fi
grep -q 'missing its canonical operation handle' "$fixture_root/error"

# Force expiry immediately after a pushed terminal snapshot without real time.
# A response arriving beyond the original deadline must not advance the flow.
reset_fixture
{ echo "$receipt"; event 15 original-7 succeeded; } > "$fixture_responses"
echo 0 > "$fixture_root/deadline-checks"
lan_remaining() {
  local checks
  [[ "$1" == 4242 ]] || return 1
  checks=$(cat "$fixture_root/deadline-checks")
  checks=$((checks+1))
  echo "$checks" > "$fixture_root/deadline-checks"
  if ((checks >= 4)); then echo 'fixture deadline elapsed' >&2; return 1; fi
  echo 7
}
if semantic alice "$intent" 4242 > "$fixture_root/out" 2> "$fixture_root/error"; then
  echo 'late terminal response bypassed original deadline' >&2; exit 1
fi
grep -q 'fixture deadline elapsed' "$fixture_root/error"
echo 'LAN semantic driver fixtures passed'
