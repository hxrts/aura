#!/usr/bin/env bash
# Isolated private LAN request and redacted receipt fixtures; no live host.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
export AURA_E2E_ROOT="$fixture/checkout" AURA_E2E_RUN_DIR="$fixture/evidence"
export AURA_E2E_RUN_TOKEN=private-enrollment-fixture AURA_E2E_HOST_ADDR=192.0.2.10
export AURA_E2E_REMOTE=fixture-host AURA_E2E_REMOTE_ROOT='remote root'
export FIXTURE_REQUEST="$fixture/request" FIXTURE_ARGS="$fixture/args"
mkdir -p "$fixture/bin" "$AURA_E2E_ROOT"
cat > "$fixture/bin/private-driver" <<'DRIVER'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$FIXTURE_ARGS"
cat > "$FIXTURE_REQUEST"
printf '%s\n' '{"status":"ok","payload":{"setup_code":"private-setup-code-fixture"}}'
DRIVER
cat > "$fixture/bin/ssh" <<'SSH'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$FIXTURE_ARGS"
remote_command=${*: -1}
[[ "$remote_command" == *'/nix/var/nix/profiles/default/bin/nix develop --command scripts/harness/lan/drv.sh req'* ]] || {
  echo 'remote request bypassed pinned Nix tool environment' >&2; exit 127
}
cat > "$FIXTURE_REQUEST"
printf '%s\n' '{"status":"ok","payload":{"setup_code":"private-setup-code-fixture"}}'
SSH
chmod +x "$fixture/bin/private-driver" "$fixture/bin/ssh"
source "$here/lib.sh"
AURA_E2E_DRV="$fixture/bin/private-driver"
secret=private-enrollment-code-fixture
request="{\"method\":\"submit_semantic_command\",\"params\":{\"intent\":{\"ImportDeviceEnrollmentCode\":{\"code\":\"$secret\"}}}}"
printf '%s\n' "$request" | PATH="$fixture/bin:$PATH" req_private alex-tui 7 > "$fixture/local-response"
jq -e --arg expected "$secret" '.params.instance_id=="alex-tui" and .params.intent.ImportDeviceEnrollmentCode.code==$expected' "$FIXTURE_REQUEST" >/dev/null
[[ $(cat "$FIXTURE_ARGS") == req-private ]]
! rg -Fq "$secret" "$FIXTURE_ARGS" "$fixture/local-response"
printf '%s\n' "$request" | PATH="$fixture/bin:$PATH" req_private barbara-tui 7 > "$fixture/remote-response"
jq -e --arg expected "$secret" '.params.instance_id=="barbara-tui" and .params.intent.ImportDeviceEnrollmentCode.code==$expected' "$FIXTURE_REQUEST" >/dev/null
! rg -Fq "$secret" "$FIXTURE_ARGS" "$fixture/remote-response"
rg -q 'nix develop --command scripts/harness/lan/drv.sh req-private' "$FIXTURE_ARGS"
rg -Fq 'AURA_E2E_RUN_TOKEN=private-enrollment-fixture' "$FIXTURE_ARGS"
PATH="$fixture/bin:$PATH" req barbara-tui ui_state > "$fixture/remote-generic-response"
rg -q 'nix develop --command scripts/harness/lan/drv.sh req ' "$FIXTURE_ARGS"
cp "$FIXTURE_ARGS" "$fixture/remote-args-before-invalid-token"
if (
  export AURA_E2E_RUN_TOKEN='invalid token'
  printf '%s\n' "$request" | PATH="$fixture/bin:$PATH" req_private barbara-tui 7 > "$fixture/invalid-token-response"
); then
  echo 'private request accepted an invalid run token' >&2; exit 1
fi
cmp -s "$FIXTURE_ARGS" "$fixture/remote-args-before-invalid-token"

# Whitelist the exact operation and host acknowledgment while excluding code,
# diagnostic detail, labels and unrelated snapshot fields.
mkdir -m 700 "$AURA_E2E_RUN_DIR"
cat > "$fixture/snapshot" <<'RESPONSE'
{"status":"ok","payload":{"screen":"private-enrollment-code-fixture","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2","state":"failed","failure":{"domain":"transport","code":"unavailable","detail":"private-enrollment-code-fixture"}}],"runtime_events":[{"id":"tui-runtime-fact-private-enrollment-code-fixture","fact":{"kind":"device_enrollment_accepted","device_id":"00000000-0000-4000-8000-000000000002","device_count":2,"code":"private-enrollment-code-fixture"}}],"lists":[{"id":"devices","items":[{"id":"private-enrollment-code-fixture","label":"private-enrollment-code-fixture"}]}],"diagnostic":"private-enrollment-code-fixture"}}
RESPONSE
evidence=$(lan_private_receipt barbara-tui ui_state pre-restart < "$fixture/snapshot")
jq -e '.operations[0]=={id:"device_enrollment",instance_id:"tui-op-device_enrollment-2",state:"failed",failure:{domain:"transport",code:"unavailable"}} and .host_acknowledgments[0]=={device_id:"00000000-0000-4000-8000-000000000002",device_count:2} and .ready==true' "$evidence" >/dev/null
[[ $(stat -c '%a' "$evidence" 2>/dev/null || stat -f '%Lp' "$evidence") == 600 ]]
! rg -Fq "$secret" "$evidence"
! lan_private_receipt barbara-tui ui_state pre-restart < "$fixture/snapshot"
ln -s "$fixture/snapshot" "$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/barbara-tui-symlink.json"
! lan_private_receipt barbara-tui ui_state symlink < "$fixture/snapshot"
for path in '.status' '.payload.readiness' '.payload.operations[0].id' '.payload.operations[0].instance_id' '.payload.operations[0].state' '.payload.operations[0].failure.domain' '.payload.operations[0].failure.code' '.payload.runtime_events[0].id' '.payload.runtime_events[0].fact.device_id' '.payload.runtime_events[0].fact.device_count'; do
  jq -c --arg secret "$secret" "$path=\$secret" "$fixture/snapshot" > "$fixture/poisoned"
  if candidate=$(lan_private_receipt barbara-tui ui_state "bad-$RANDOM" < "$fixture/poisoned"); then
    if rg -Fq "$secret" "$candidate"; then
      echo "private receipt leaked secret from $path" >&2; exit 1
    fi
  fi
done
printf '%s\n' '{"status":"ok","payload":{"submission":"accepted","handle":{"ui_operation":{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2"}},"value":{"code":"private-enrollment-code-fixture"}}}' \
  | lan_private_receipt barbara-tui submit_semantic_command submitted >/dev/null
printf '%s\n' '{"status":"ok","payload":{"status":"restarted","message":"private-enrollment-code-fixture"}}' \
  | lan_private_receipt barbara-tui restart reopened >/dev/null
browser_snapshot=$(jq -c '.payload.operations[0].instance_id="op-2" |
  .payload.runtime_events[0].id="runtime-event-9"' "$fixture/snapshot")
browser_receipt=$(printf '%s\n' "$browser_snapshot" |
  lan_private_receipt barbara-web ui_state browser-before-restart)
jq -e '.operations[0].instance_id=="op-2" and
  .operations[0].failure=={domain:"transport",code:"unavailable"} and
  .host_acknowledgments[0].device_count==2 and .tool_failure==null' "$browser_receipt" >/dev/null
for case_name in structured unavailable timeout closed decode unknown; do
  case "$case_name" in
    structured) prefix='Playwright driver ui_state failed: Error: structured ui_state observation failed for instance barbara-web'; expected=browser_structured_observation_failed;;
    unavailable) prefix='Playwright driver ui_state failed: Error: browser UI state unavailable for instance barbara-web'; expected=browser_ui_state_unavailable;;
    timeout) prefix='Playwright driver ui_state timed out'; expected=browser_driver_timeout;;
    closed) prefix='Playwright driver closed stdout while awaiting ui_state'; expected=browser_driver_closed;;
    decode) prefix='failed to decode browser UiSnapshot'; expected=browser_snapshot_decode_failed;;
    unknown) prefix='unexpected browser error'; expected=unclassified;;
  esac
  error_receipt=$(jq -cn --arg message "$prefix: $secret" '{status:"error",message:$message}' |
    lan_private_receipt barbara-web ui_state "browser-error-$case_name")
  jq -e --arg expected "$expected" '.status=="error" and
    .tool_failure=={domain:"tool_api",code:$expected} and .operations==[] and
    .host_acknowledgments==[]' "$error_receipt" >/dev/null
  ! rg -Fq "$secret" "$error_receipt"
done
! printf '%s\n' '{"status":"error","message":{"secret":"private-enrollment-code-fixture"}}' |
  lan_private_receipt barbara-web ui_state browser-error-malformed >/dev/null
[[ $(stat -c '%a' "$browser_receipt" 2>/dev/null || stat -f '%Lp' "$browser_receipt") == 600 ]]
browser_event=$(jq -cn --argjson snapshot "$(printf '%s\n' "$browser_snapshot" | jq -c .payload)" \
  '{status:"ok",payload:{event:{version:9,snapshot:$snapshot}}}')
event_receipt=$(printf '%s\n' "$browser_event" |
  lan_private_receipt barbara-web wait_for_ui_snapshot_event browser-accepted-event)
jq -e '.operations[0].instance_id=="op-2" and
  .host_acknowledgments[0].device_count==2' "$event_receipt" >/dev/null
[[ ! -e "$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/barbara-web-browser-accepted-event.raw-private.json" ]]
original_root=$AURA_E2E_ROOT
AURA_E2E_ROOT=$(cd "$here/../../.." && pwd)
source_revision=$(git -C "$AURA_E2E_ROOT" rev-parse HEAD)
failed_event=$(printf '%s\n' "$browser_event" | jq -c --arg secret "$secret" '
  .payload.event.snapshot.operations[0].failure={domain:"invitation",code:"internal_error",detail:$secret}')
failure_receipt=$(printf '%s\n' "$failed_event" |
  lan_private_receipt barbara-web wait_for_ui_snapshot_event original-failed op-2)
failure_raw="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/barbara-web-original-failed.failure-raw-private.json"
failure_custody="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/barbara-web-original-failed.failure-custody.json"
jq -e '.operations[0].instance_id=="op-2" and .operations[0].state=="failed" and
  .operations[0].failure=={domain:"invitation",code:"internal_error"}' "$failure_receipt" >/dev/null
jq -e --arg source "$source_revision" --arg token "$AURA_E2E_RUN_TOKEN" \
  --arg hash "$(sha256sum "$failure_raw" | cut -d' ' -f1)" \
  --arg tooling_hash "$(sha256sum "$here/lib.sh" | cut -d' ' -f1)" '
  .source_revision==$source and .run_token==$token and .instance_id=="barbara-web" and
  .method=="wait_for_ui_snapshot_event" and .label=="original-failed" and
  .operation_instance=="op-2" and .raw_response_sha256==$hash and
  .raw_response_file=="barbara-web-original-failed.failure-raw-private.json" and
  .redacted_receipt_file=="barbara-web-original-failed.json" and
  .tooling.lib_sha256==$tooling_hash and .tooling.committed_bundle_verified==false and
  .raw_response_custody=="owner_only_private_artifact"' "$failure_custody" >/dev/null
rg -Fq "$secret" "$failure_raw"
! rg -Fq "$secret" "$failure_receipt" "$failure_custody"
for path in "$failure_raw" "$failure_custody" "$failure_receipt"; do
  [[ $(stat -c '%a' "$path" 2>/dev/null || stat -f '%Lp' "$path") == 600 ]]
done
foreign_failed=$(printf '%s\n' "$failed_event" | jq -c '.payload.event.snapshot.operations[0].instance_id="op-3"')
! printf '%s\n' "$foreign_failed" |
  lan_private_receipt barbara-web wait_for_ui_snapshot_event foreign-failed op-2 \
  > "$fixture/foreign-failed.stdout" 2> "$fixture/foreign-failed.stderr"
[[ ! -e "$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/barbara-web-foreign-failed.failure-raw-private.json" ]]
! rg -Fq "$secret" "$fixture/foreign-failed.stdout" "$fixture/foreign-failed.stderr"
for collision in raw custody; do
  collision_base="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/barbara-web-collision-$collision"
  if [[ "$collision" == raw ]]; then
    ln -s "$fixture/snapshot" "$collision_base.failure-raw-private.json"
    protected="$collision_base.failure-raw-private.json"
  else
    printf 'existing-owner-artifact\n' > "$collision_base.failure-custody.json"
    protected="$collision_base.failure-custody.json"
  fi
  ! printf '%s\n' "$failed_event" |
    lan_private_receipt barbara-web wait_for_ui_snapshot_event "collision-$collision" op-2 \
    > "$fixture/collision-$collision.stdout" 2> "$fixture/collision-$collision.stderr"
  [[ ! -e "$collision_base.json" ]]
  if [[ "$collision" == raw ]]; then [[ -L "$protected" ]];
  else [[ $(cat "$protected") == existing-owner-artifact ]]; fi
  ! rg -Fq "$secret" "$fixture/collision-$collision.stdout" "$fixture/collision-$collision.stderr"
done
mkdir "$fixture/race-bin"
cat > "$fixture/race-bin/ln" <<'RACE_LN'
#!/usr/bin/env bash
destination=${*: -1}
if [[ "$destination" == *'/barbara-web-collision-race.failure-custody.json' ]]; then
  printf 'raced-existing-owner-artifact\n' > "$destination"
fi
exec /bin/ln "$@"
RACE_LN
chmod +x "$fixture/race-bin/ln"
race_base="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/barbara-web-collision-race"
! printf '%s\n' "$failed_event" |
  PATH="$fixture/race-bin:$PATH" lan_private_receipt barbara-web wait_for_ui_snapshot_event collision-race op-2 \
  > "$fixture/collision-race.stdout" 2> "$fixture/collision-race.stderr"
[[ $(cat "$race_base.failure-custody.json") == raced-existing-owner-artifact ]]
[[ ! -e "$race_base.failure-raw-private.json" && ! -e "$race_base.json" ]]
! rg -Fq "$secret" "$fixture/collision-race.stdout" "$fixture/collision-race.stderr"
AURA_E2E_ROOT=$original_root
numeric_ack=$(printf '%s\n' "$browser_event" | jq -c '.payload.event.snapshot.runtime_events[0].id=9')
! printf '%s\n' "$numeric_ack" |
  lan_private_receipt barbara-web wait_for_ui_snapshot_event browser-numeric-ack >/dev/null
failure_root="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN"
jq -e '.validation_failure=="invalid_host_ack_id" and
  .raw_response_custody=="owner_only_private_artifact"' \
  "$failure_root/barbara-web-browser-numeric-ack.validation.json" >/dev/null
[[ $(stat -c '%a' "$failure_root/barbara-web-browser-numeric-ack.raw-private.json" 2>/dev/null ||
      stat -f '%Lp' "$failure_root/barbara-web-browser-numeric-ack.raw-private.json") == 600 ]]
! rg -Fq "$secret" "$failure_root/barbara-web-browser-numeric-ack.validation.json"
! printf '%s\n' "$browser_event" | jq -c \
  '.payload.event.snapshot.runtime_events[0].id="runtime-event-foreign"' |
  lan_private_receipt barbara-web wait_for_ui_snapshot_event browser-foreign-ack >/dev/null
malformed_ack=$(printf '%s\n' "$browser_event" | jq -c --arg secret "$secret" \
  '.payload.event.snapshot.runtime_events[0].id={unexpected:$secret}')
! printf '%s\n' "$malformed_ack" |
  lan_private_receipt barbara-web wait_for_ui_snapshot_event browser-malformed-ack \
  > "$fixture/malformed-stdout" 2> "$fixture/malformed-stderr"
jq -e '.validation_failure=="invalid_host_ack_id"' \
  "$failure_root/barbara-web-browser-malformed-ack.validation.json" >/dev/null
rg -Fq "$secret" "$failure_root/barbara-web-browser-malformed-ack.raw-private.json"
! rg -Fq "$secret" "$fixture/malformed-stdout" "$fixture/malformed-stderr" \
  "$failure_root/barbara-web-browser-malformed-ack.validation.json"
oversized="$fixture/oversized-response"
printf '%s' "$browser_snapshot" > "$oversized"
head -c 1048577 /dev/zero | tr '\0' ' ' >> "$oversized"
! lan_private_receipt barbara-web ui_state browser-oversized \
  < "$oversized" > "$fixture/oversized-stdout" 2> "$fixture/oversized-stderr"
oversized_prefix="$failure_root/barbara-web-browser-oversized.raw-prefix-private.bin"
oversized_reason="$failure_root/barbara-web-browser-oversized.validation.json"
jq -e --arg token "$AURA_E2E_RUN_TOKEN" \
  '.run_token==$token and .instance_id=="barbara-web" and .method=="ui_state" and
   .label=="browser-oversized" and .validation_failure=="response_too_large" and
   .capture_limit_bytes==1048576 and .captured_prefix_bytes==1048576 and
   .truncated==true and .raw_response_custody=="owner_only_bounded_prefix"' \
  "$oversized_reason" >/dev/null
[[ $(wc -c < "$oversized_prefix") == 1048576 ]]
[[ $(stat -c '%a' "$oversized_prefix" 2>/dev/null || stat -f '%Lp' "$oversized_prefix") == 600 ]]
[[ $(stat -c '%a' "$oversized_reason" 2>/dev/null || stat -f '%Lp' "$oversized_reason") == 600 ]]
rg -Fq "$secret" "$oversized_prefix"
! rg -Fq "$secret" "$fixture/oversized-stdout" "$fixture/oversized-stderr" "$oversized_reason"
[[ ! -e "$failure_root/barbara-web-browser-oversized.raw-private.json" ]]
ln -s "$fixture/snapshot" "$failure_root/barbara-web-oversized-link.raw-prefix-private.bin"
! lan_private_receipt barbara-web ui_state oversized-link < "$oversized" >/dev/null
[[ -L "$failure_root/barbara-web-oversized-link.raw-prefix-private.bin" ]]
[[ ! -e "$failure_root/barbara-web-oversized-link.validation.json" ]]
collision_reason="$failure_root/barbara-web-oversized-collision.validation.json"
printf '%s\n' 'preexisting-owner-receipt' > "$collision_reason"
! lan_private_receipt barbara-web ui_state oversized-collision < "$oversized" >/dev/null
[[ $(cat "$collision_reason") == preexisting-owner-receipt ]]
[[ ! -e "$failure_root/barbara-web-oversized-collision.raw-prefix-private.bin" ]]
! printf '%s\n' "$browser_event" | jq -c \
  '.payload.event.snapshot.operations[0].instance_id="foreign-op-2"' |
  lan_private_receipt barbara-web wait_for_ui_snapshot_event browser-foreign-operation >/dev/null
! printf '%s\n' "$browser_snapshot" | lan_private_receipt barbara-tui ui_state browser-as-tui >/dev/null
! printf '%s\n' "$browser_snapshot" | jq -c '.payload.runtime_events[0].id="tui-runtime-fact-forged"' |
  lan_private_receipt barbara-web ui_state browser-forged-ack >/dev/null
! printf '%s\n' "$browser_snapshot" | jq -c '.payload.operations[0].instance_id="tui-op-device_enrollment-2"' |
  lan_private_receipt barbara-web ui_state browser-forged-operation >/dev/null
! rg -Fq "$secret" "$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN" --glob '*.json'
# A private tooling bundle must match all three committed load-closure files,
# independently of the candidate checkout named in the failure receipt.
tooling_source="$fixture/tooling-source"
tooling_bundle="$fixture/tooling-bundle"
mkdir -p "$tooling_source/scripts/harness/lan" "$tooling_bundle/scripts/harness/lan"
chmod 700 "$tooling_bundle"
for name in paired-enrollment.sh lib.sh env.sh; do
  cp "$here/$name" "$tooling_source/scripts/harness/lan/$name"
  cp "$here/$name" "$tooling_bundle/scripts/harness/lan/$name"
done
git -C "$tooling_source" init -q
git -C "$tooling_source" -c user.name=Fixture -c user.email=fixture@example.invalid \
  add scripts/harness/lan
git -C "$tooling_source" -c user.name=Fixture -c user.email=fixture@example.invalid \
  commit -qm 'fixture tooling closure'
tooling_commit=$(git -C "$tooling_source" rev-parse HEAD)
jq -cn --arg commit "$tooling_commit" \
  --arg paired "$(sha256sum "$here/paired-enrollment.sh" | cut -d' ' -f1)" \
  --arg lib "$(sha256sum "$here/lib.sh" | cut -d' ' -f1)" \
  --arg env "$(sha256sum "$here/env.sh" | cut -d' ' -f1)" '
  {tooling_commit:$commit,files:{"paired-enrollment.sh":$paired,"lib.sh":$lib,"env.sh":$env}}' \
  > "$tooling_bundle/provenance.json"
chmod 600 "$tooling_bundle/provenance.json"
AURA_E2E_ROOT=$tooling_source
source "$tooling_bundle/scripts/harness/lan/lib.sh"
lan_private_tooling_identity | jq -e --arg commit "$tooling_commit" '
  .revision==$commit and .committed_bundle_verified==true and
  (.lib_sha256|type=="string" and length==64)' >/dev/null
bundle_failure_receipt=$(printf '%s\n' "$failed_event" |
  lan_private_receipt barbara-web wait_for_ui_snapshot_event bundle-failed op-2)
bundle_custody="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/barbara-web-bundle-failed.failure-custody.json"
jq -e --arg commit "$tooling_commit" \
  --arg hash "$(sha256sum "$tooling_bundle/scripts/harness/lan/lib.sh" | cut -d' ' -f1)" '
  .tooling.revision==$commit and .tooling.lib_sha256==$hash and
  .tooling.committed_bundle_verified==true and .operation_instance=="op-2"' \
  "$bundle_custody" >/dev/null
! rg -Fq "$secret" "$bundle_failure_receipt" "$bundle_custody"
printf '\n' >> "$tooling_bundle/scripts/harness/lan/env.sh"
! lan_private_tooling_identity > "$fixture/tampered-tooling.stdout" 2> "$fixture/tampered-tooling.stderr"
[[ ! -s "$fixture/tampered-tooling.stdout" ]]
echo 'LAN private enrollment request and redacted receipt fixtures passed'
