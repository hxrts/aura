#!/usr/bin/env bash
# Deterministic two-actor ToolApi fixture; no LAN host is contacted.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d)
trap '[[ "${KEEP_FIXTURE:-0}" == 1 ]] || rm -rf "$fixture"' EXIT
mkdir -m 700 "$fixture/evidence"
mkdir "$fixture/bin"
export AURA_E2E_ROOT="$fixture/checkout"
mkdir -p "$AURA_E2E_ROOT/.tmp/lan-checklist-paired-enrollment-fixture"
export AURA_E2E_RUN_DIR="$fixture/evidence"
export AURA_E2E_RUN_TOKEN=paired-enrollment-fixture
export AURA_E2E_REMOTE=fixture-host AURA_E2E_REMOTE_ROOT=fixture-root
export AURA_E2E_HOST_ADDR=192.0.2.10 AURA_E2E_BOOTSTRAP=paired-enrollment
export AURA_E2E_DRV="$fixture/bin/driver" FIXTURE_LOG="$fixture/requests"
export FIXTURE_DEVICES_OPEN="$fixture/devices-open"
export FIXTURE_APPROVED="$fixture/approved"
export AURA_E2E_PRIVATE_WAIT_SECONDS=2
export AURA_E2E_AURA_BIN="$fixture/bin/aura" AURA_E2E_TOOL_REPL="$fixture/bin/tool_repl"
export AURA_E2E_WEB_INDEX="$fixture/web-index.html"
export AURA_E2E_LOCAL_CONFIG="$fixture/broker.toml"
printf '  "AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=%064d",\n' 0 > "$AURA_E2E_LOCAL_CONFIG"
chmod 600 "$AURA_E2E_LOCAL_CONFIG"
export FIXTURE_COMMIT=$(git -C "$here/../../.." rev-parse HEAD)
printf '{"phase":"preflight","commit":{"local":"%s","remote":"%s"},"run_token":"%s","network_delivery_asserted":false,"planned_process_identity":{"local":{"host":"fixture-local"},"remote":{"host":"fixture-remote"}},"broker":{"endpoint":"http://192.0.2.10:43190","policy":"development_only_lan_http"}}\n' \
  "$FIXTURE_COMMIT" "$FIXTURE_COMMIT" "$AURA_E2E_RUN_TOKEN" > \
  "$AURA_E2E_ROOT/.tmp/lan-checklist-$AURA_E2E_RUN_TOKEN/paired-preflight.json"
cat > "$fixture/bin/git" <<'GIT'
#!/usr/bin/env bash
printf '%s\n' "$FIXTURE_COMMIT"
GIT
printf 'fixture aura\n' > "$AURA_E2E_AURA_BIN"
printf 'fixture repl\n' > "$AURA_E2E_TOOL_REPL"
printf 'fixture web\n' > "$AURA_E2E_WEB_INDEX"
printf '{"pid":100,"birth":"fixture-local","run_token":"%s"}\n' "$AURA_E2E_RUN_TOKEN" > "$AURA_E2E_RUN_DIR/repl.identity.json"
cat > "$fixture/bin/ssh" <<'SSH'
#!/usr/bin/env bash
case "$*" in
  *'git rev-parse HEAD'*) printf '%s\n' "$FIXTURE_COMMIT"; exit 0;;
  *'repl.identity.json'*) printf '{"pid":101,"birth":"fixture-remote","run_token":"%s"}\n' "$AURA_E2E_RUN_TOKEN"; exit 0;;
esac
exec "$AURA_E2E_DRV" req-private
SSH
cat > "$fixture/bin/driver" <<'DRIVER'
#!/usr/bin/env bash
if [[ "$1" == inspect-enrollment-manifest ]]; then
  jq -e '.manifest_code=="signed-manifest-secret"' >/dev/null || exit 1
  echo '{"subject":"authority-11111111-1111-1111-1111-111111111111","device":"22222222-2222-2222-2222-222222222222","manifest_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}'
  exit 0
fi
[[ "$1" == req-private ]] || exit 2
IFS= read -r request
printf '%s\n' "$request" >> "$FIXTURE_LOG"
instance=$(jq -r .params.instance_id <<< "$request")
method=$(jq -r .method <<< "$request")
intent=$(jq -r '.params.intent|keys[0] // ""' <<< "$request")
case "$instance:$method:$intent" in
  barbara-tui:prepare_device_enrollment_setup:)
    echo '{"status":"ok","payload":{"setup_code":"signed-setup-secret"}}' ;;
  alex-tui:submit_semantic_command:OpenScreen)
    echo '{"status":"ok","payload":{"submission":"accepted"}}' ;;
  alex-tui:submit_semantic_command:StartDeviceEnrollment)
    jq -e '.params.intent.StartDeviceEnrollment.setup_code=="signed-setup-secret"' <<< "$request" >/dev/null || exit 1
    echo '{"status":"ok","payload":{"submission":"accepted","handle":{"ui_operation":{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-1"}}}}' ;;
  alex-tui:wait_for_ui_snapshot_event:)
    if [[ $(jq -r '.params.after_version' <<< "$request") == null ]]; then
      echo '{"status":"ok","payload":{"event":{"version":1,"snapshot":{"screen":"settings","readiness":"ready","quiescence":{"state":"settled"},"operations":[],"runtime_events":[]}}}}'
    else
      echo '{"status":"ok","payload":{"event":{"version":2,"snapshot":{"screen":"settings","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-1","state":"succeeded","failure":null}],"runtime_events":[{"id":"tui-runtime-fact-secret","fact":{"kind":"device_enrollment_code_ready","code":"issued-code-secret","manifest_transfer":{"manifest_code":"signed-manifest-secret","initiator_verifier_code":"retained-verifier-secret"}}}]}}}}'
    fi ;;
  barbara-tui:ui_state:)
    if [[ "${FIXTURE_CASE:-}" == no_network_ack && -e "$FIXTURE_APPROVED" ]]; then
      echo '{"status":"ok","payload":{"screen":"neighborhood","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2","state":"failed","failure":{"domain":"ceremony","code":"operation_timed_out"}}],"runtime_events":[]}}'
      exit 0
    fi
    echo '{"status":"ok","payload":{"screen":"neighborhood","readiness":"ready","operations":[],"runtime_events":[]}}' ;;
  barbara-tui:submit_semantic_command:ImportDeviceEnrollmentCode)
    jq -e '.params.intent.ImportDeviceEnrollmentCode=={code:"issued-code-secret",manifest_transfer:{manifest_code:"signed-manifest-secret",initiator_verifier_code:"retained-verifier-secret"}}' <<< "$request" >/dev/null || exit 1
    : > "$FIXTURE_APPROVED"
    echo '{"status":"ok","payload":{"submission":"accepted","handle":{"ui_operation":{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2"}}}}' ;;
  barbara-tui:submit_semantic_command:OpenSettingsSection)
    jq -e '.params.intent.OpenSettingsSection=="devices"' <<< "$request" >/dev/null || exit 1
    : > "$FIXTURE_DEVICES_OPEN"
    echo '{"status":"ok","payload":{"submission":"accepted"}}' ;;
  barbara-tui:wait_for_ui_snapshot_event:)
    if [[ "${FIXTURE_CASE:-}" == missing_ack ]]; then
      echo '{"status":"ok","payload":{"event":{"version":3,"snapshot":{"screen":"neighborhood","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2","state":"succeeded","failure":null}],"runtime_events":[],"lists":[{"id":"devices","items":[]}]}}}}'
      exit 0
    fi
    if [[ -e "$FIXTURE_RESTARTED" && ! -e "$FIXTURE_DEVICES_OPEN" ]]; then
      echo '{"status":"ok","payload":{"event":{"version":4,"snapshot":{"screen":"neighborhood","readiness":"ready","operations":[],"runtime_events":[],"lists":[]}}}}'
      exit 0
    fi
    if [[ "${FIXTURE_CASE:-}" == wrong_reopen_device && -e "$FIXTURE_RESTARTED" ]]; then
      echo '{"status":"ok","payload":{"event":{"version":5,"snapshot":{"screen":"settings","readiness":"ready","operations":[],"runtime_events":[],"lists":[{"id":"devices","items":[{"id":"00000000-0000-4000-8000-000000000001"},{"id":"00000000-0000-4000-8000-000000000003"}]}]}}}}'
      exit 0
    fi
    if [[ "${FIXTURE_CASE:-}" == missing_accepted_list && -e "$FIXTURE_DEVICES_OPEN" && ! -e "$FIXTURE_RESTARTED" ]]; then
      echo '{"status":"ok","payload":{"event":{"version":5,"snapshot":{"screen":"settings","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2","state":"succeeded","failure":null}],"runtime_events":[{"id":"tui-runtime-fact-accepted","fact":{"kind":"device_enrollment_accepted","device_id":"00000000-0000-4000-8000-000000000001","device_count":2}}],"lists":[]}}}}'
      exit 0
    fi
    if [[ -e "$FIXTURE_DEVICES_OPEN" ]]; then
      echo '{"status":"ok","payload":{"event":{"version":5,"snapshot":{"screen":"settings","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2","state":"succeeded","failure":null}],"runtime_events":[{"id":"tui-runtime-fact-accepted","fact":{"kind":"device_enrollment_accepted","device_id":"00000000-0000-4000-8000-000000000001","device_count":2}}],"lists":[{"id":"devices","items":[{"id":"00000000-0000-4000-8000-000000000001"},{"id":"22222222-2222-2222-2222-222222222222"}]}]}}}}'
      exit 0
    fi
    if [[ "${FIXTURE_CASE:-}" == wrong_host_ack_device ]]; then
      echo '{"status":"ok","payload":{"event":{"version":3,"snapshot":{"screen":"neighborhood","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2","state":"succeeded","failure":null}],"runtime_events":[{"id":"tui-runtime-fact-accepted","fact":{"kind":"device_enrollment_accepted","device_id":"00000000-0000-4000-8000-000000000003","device_count":2}}],"lists":[]}}}}'
      exit 0
    fi
    echo '{"status":"ok","payload":{"event":{"version":3,"snapshot":{"screen":"neighborhood","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2","state":"succeeded","failure":null}],"runtime_events":[{"id":"tui-runtime-fact-accepted","fact":{"kind":"device_enrollment_accepted","device_id":"00000000-0000-4000-8000-000000000001","device_count":2}}],"lists":[]}}}}' ;;
  barbara-tui:get_authority_id:)
    if [[ ! -e "$FIXTURE_RESTARTED" ]]; then
      echo '{"status":"ok","payload":{"authority_id":"authority-99999999-9999-9999-9999-999999999999","source":"backend"}}'
    elif [[ "${FIXTURE_CASE:-}" == wrong_authority ]]; then
      echo '{"status":"ok","payload":{"authority_id":"authority-88888888-8888-8888-8888-888888888888","source":"backend"}}'
    else
      echo '{"status":"ok","payload":{"authority_id":"authority-11111111-1111-1111-1111-111111111111","source":"backend"}}'
    fi ;;
  alex-tui:get_authority_id:)
    echo '{"status":"ok","payload":{"authority_id":"authority-11111111-1111-1111-1111-111111111111","source":"backend"}}' ;;
  barbara-tui:restart:)
    jq -e '.params.expected_operation_instance==null' <<< "$request" >/dev/null || exit 1
    : > "$FIXTURE_RESTARTED"
    rm -f "$FIXTURE_DEVICES_OPEN"
    echo '{"status":"ok","payload":{"status":"restarted"}}' ;;
  *) echo '{"status":"error","message":"unexpected fixture request"}' ;;
esac
DRIVER
cat > "$fixture/bin/curl" <<'CURL'
#!/usr/bin/env bash
[[ "$*" == *'--config -'* ]] || exit 2
config=$(cat)
[[ "$config" == *'Authorization: Bearer 0000000000000000000000000000000000000000000000000000000000000000'* ]] || exit 2
[[ "$config" == *'/enrollment-mailbox/'*'/authority-11111111-1111-1111-1111-111111111111/22222222-2222-2222-2222-222222222222/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'* ]] || exit 2
if [[ "$config" == *'/progress-witness/'* ]]; then
  if [[ "${FIXTURE_CASE:-}" == no_registration ]]; then
    echo '{"subject":"authority-11111111-1111-1111-1111-111111111111","device":"22222222-2222-2222-2222-222222222222","manifest_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","registration_present":false,"first_message_id":null,"messages":[]}'
    exit 0
  fi
  if [[ "${FIXTURE_CASE:-}" == multi_message ]]; then
    echo '{"subject":"authority-11111111-1111-1111-1111-111111111111","device":"22222222-2222-2222-2222-222222222222","manifest_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","registration_present":true,"first_message_id":"33333333-3333-3333-3333-333333333333","messages":[{"message_id":"11111111-1111-1111-1111-111111111111","envelope_digest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","stage":"receiver_acknowledged_retained_frame"},{"message_id":"33333333-3333-3333-3333-333333333333","envelope_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","stage":"retrieval_selected_for_receiver_credential"}]}'
    exit 0
  fi
  echo '{"subject":"authority-11111111-1111-1111-1111-111111111111","device":"22222222-2222-2222-2222-222222222222","manifest_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","registration_present":true,"first_message_id":"33333333-3333-3333-3333-333333333333","messages":[{"message_id":"33333333-3333-3333-3333-333333333333","envelope_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","stage":"retrieval_selected_for_receiver_credential"}]}'
  exit 0
fi
[[ "$config" == *'/ack-witness/'* ]] || exit 2
case "${FIXTURE_CASE:-}" in
  no_network_ack) echo '[]';;
  no_registration) echo '[]';;
  wrong_network_message_id) echo '[{"subject":"authority-11111111-1111-1111-1111-111111111111","device":"22222222-2222-2222-2222-222222222222","manifest_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","message_id":"44444444-4444-4444-4444-444444444444","envelope_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","stage":"receiver_acknowledged_retained_frame"}]';;
  multi_message) echo '[{"subject":"authority-11111111-1111-1111-1111-111111111111","device":"22222222-2222-2222-2222-222222222222","manifest_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","message_id":"11111111-1111-1111-1111-111111111111","envelope_digest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","stage":"receiver_acknowledged_retained_frame"},{"subject":"authority-11111111-1111-1111-1111-111111111111","device":"22222222-2222-2222-2222-222222222222","manifest_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","message_id":"33333333-3333-3333-3333-333333333333","envelope_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","stage":"receiver_acknowledged_retained_frame"}]';;
  wrong_network_digest) echo '[{"subject":"authority-11111111-1111-1111-1111-111111111111","device":"22222222-2222-2222-2222-222222222222","manifest_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","message_id":"33333333-3333-3333-3333-333333333333","envelope_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","stage":"receiver_acknowledged_retained_frame"}]';;
  *) echo '[{"subject":"authority-11111111-1111-1111-1111-111111111111","device":"22222222-2222-2222-2222-222222222222","manifest_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","message_id":"33333333-3333-3333-3333-333333333333","envelope_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","stage":"receiver_acknowledged_retained_frame"}]';;
esac
CURL
chmod +x "$fixture/bin/ssh" "$fixture/bin/driver" "$fixture/bin/git" "$fixture/bin/curl"
# Keep the fake driver anchored to the actual shared serde contract. The old
# uppercase spelling passed this fake while real ToolApi rejected it.
rg -Uq '#\[serde\(rename_all = "snake_case"\)\][[:space:]]+pub enum SettingsSection \{[[:space:]]+Devices' \
  "$here/../../../crates/aura-app/src/scenario_contract/actions.rs"
if printf '%s\n' '{"method":"submit_semantic_command","params":{"instance_id":"barbara-tui","intent":{"OpenSettingsSection":"Devices"}}}' |
    "$fixture/bin/driver" req-private >/dev/null 2>&1; then
  echo 'fake driver accepted the noncanonical settings variant' >&2; exit 1
fi
export FIXTURE_RESTARTED="$fixture/restarted"
PATH="$fixture/bin:$PATH" bash "$here/paired-enrollment.sh" > "$fixture/stdout" 2> "$fixture/stderr"
manifest="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/paired-result.json"
jq -e '.assertions.exact_operation_and_host_ack and .assertions.same_device_after_reopen and .transport.network_delivery_asserted==true and .evidence.network_delivery.message_id=="33333333-3333-3333-3333-333333333333"' "$manifest" >/dev/null
jq -e '.network_delivery_asserted==true and
  .operation_instance=="tui-op-device_enrollment-2" and
  .signed_manifest_device=="22222222-2222-2222-2222-222222222222" and
  .original_ack.message_id=="33333333-3333-3333-3333-333333333333"' \
  "$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/broker-ack.json" >/dev/null
jq -es '
  [.[]|select(.params.instance_id=="barbara-tui" and
    .method=="submit_semantic_command" and
    .params.intent.OpenSettingsSection=="devices")]|length==2' "$FIXTURE_LOG" >/dev/null
for secret in signed-setup-secret issued-code-secret signed-manifest-secret retained-verifier-secret; do
  ! rg -Fq "$secret" "$fixture/stdout" "$fixture/stderr" "$AURA_E2E_RUN_DIR"
  rg -Fq "$secret" "$FIXTURE_LOG"
done
multi_dir="$fixture/multi-message"
mkdir -m 700 "$multi_dir"
printf '{"pid":100,"birth":"fixture-local","run_token":"%s"}\n' "$AURA_E2E_RUN_TOKEN" > "$multi_dir/repl.identity.json"
FIXTURE_CASE=multi_message AURA_E2E_RUN_DIR="$multi_dir" PATH="$fixture/bin:$PATH" \
  bash "$here/paired-enrollment.sh" > "$fixture/multi.stdout" 2> "$fixture/multi.stderr"
jq -e '.evidence.network_delivery.message_id=="33333333-3333-3333-3333-333333333333" and
  .transport.network_delivery_asserted==true' \
  "$multi_dir/private-enrollment-$AURA_E2E_RUN_TOKEN/paired-result.json" >/dev/null
for failure in missing_ack wrong_host_ack_device missing_accepted_list wrong_authority wrong_reopen_device no_network_ack no_registration wrong_network_message_id wrong_network_digest; do
  rm -f "$FIXTURE_RESTARTED" "$FIXTURE_DEVICES_OPEN" "$FIXTURE_APPROVED"
  bad_dir="$fixture/$failure"
  mkdir -m 700 "$bad_dir"
  printf '{"pid":100,"birth":"fixture-local","run_token":"%s"}\n' "$AURA_E2E_RUN_TOKEN" > "$bad_dir/repl.identity.json"
  if FIXTURE_CASE="$failure" AURA_E2E_RUN_DIR="$bad_dir" PATH="$fixture/bin:$PATH" \
      AURA_E2E_NETWORK_WITNESS_TIMEOUT=1 \
      bash "$here/paired-enrollment.sh" > "$fixture/$failure.stdout" 2> "$fixture/$failure.stderr"; then
    echo "paired fixture accepted $failure" >&2; exit 1
  fi
  [[ ! -e "$bad_dir/private-enrollment-$AURA_E2E_RUN_TOKEN/paired-result.json" ]]
  if [[ "$failure" == wrong_host_ack_device ]]; then
    jq -e '.typed_failure=="canonical_device_binding_mismatch" and
      .stage=="accepted-devices" and
      .operation_instance=="tui-op-device_enrollment-2" and
      .signed_manifest_device=="22222222-2222-2222-2222-222222222222" and
      .host_ack_device=="00000000-0000-4000-8000-000000000003" and
      (.canonical_device_ids|sort)==["00000000-0000-4000-8000-000000000001","22222222-2222-2222-2222-222222222222"]' \
      "$bad_dir/private-enrollment-$AURA_E2E_RUN_TOKEN/accepted-devices-list-mismatch.json" >/dev/null
  fi
  if [[ "$failure" == no_network_ack || "$failure" == no_registration || "$failure" == wrong_network_message_id || "$failure" == wrong_network_digest ]]; then
    stage_receipt="$bad_dir/private-enrollment-$AURA_E2E_RUN_TOKEN/broker-stage.json"
    jq -e '.network_delivery_asserted==false and (.progress_history|length)==1 and
      .progress_history[0]==.progress' "$stage_receipt" >/dev/null
    if [[ "$failure" == no_registration ]]; then
      jq -e '.first_missing_stage=="registration" and .original_message_id==null and
        .progress.registration_present==false and .progress.messages==[]' "$stage_receipt" >/dev/null
    else
      jq -e '.progress.registration_present==true and
        .progress.messages[0].stage=="retrieval_selected_for_receiver_credential" and
        .original_message_id=="33333333-3333-3333-3333-333333333333"' "$stage_receipt" >/dev/null
    fi
    if [[ "$failure" == wrong_network_message_id ]]; then
      jq -e '.first_missing_stage=="receiver_ack"' "$stage_receipt" >/dev/null
    fi
    if [[ "$failure" == no_network_ack ]]; then
      jq -e '.owner_observation.operation.instance_id=="tui-op-device_enrollment-2" and
        .owner_observation.operation.state=="failed" and
        .owner_observation.operation.failure=={domain:"ceremony",code:"operation_timed_out"} and
        .owner_observation.host_acknowledgments==[]' "$stage_receipt" >/dev/null
    fi
    [[ $(stat -c '%a' "$stage_receipt" 2>/dev/null || stat -f '%Lp' "$stage_receipt") == 600 ]]
  fi
  for secret in signed-setup-secret issued-code-secret signed-manifest-secret retained-verifier-secret; do
    ! rg -Fq "$secret" "$fixture/$failure.stdout" "$fixture/$failure.stderr" "$bad_dir"
  done
done
# A same-host diagnostic uses a second owner driver without claiming distinct
# hosts or satisfying the final paired-LAN evidence requirement.
rm -f "$FIXTURE_RESTARTED" "$FIXTURE_DEVICES_OPEN"
peer_root="$fixture/peer-checkout"
peer_run="$fixture/peer-run"
local_run="$fixture/local-run"
mkdir -p "$peer_root/scripts/harness/lan" "$peer_run" "$local_run"
cat > "$peer_root/scripts/harness/lan/drv.sh" <<'PEER'
#!/usr/bin/env bash
exec "$AURA_E2E_DRV" "$@"
PEER
chmod +x "$peer_root/scripts/harness/lan/drv.sh"
printf '{"pid":101,"birth":"fixture-peer","run_token":"%s"}\n' "$AURA_E2E_RUN_TOKEN" > "$peer_run/repl.identity.json"
printf '{"pid":100,"birth":"fixture-local","run_token":"%s"}\n' "$AURA_E2E_RUN_TOKEN" > "$local_run/repl.identity.json"
preflight="$AURA_E2E_ROOT/.tmp/lan-checklist-$AURA_E2E_RUN_TOKEN/paired-preflight.json"
jq '.phase="single-host-preflight" | .cross_host_asserted=false |
  .broker.policy="development_only_loopback_http" |
  .planned_process_identity.remote.host=.planned_process_identity.local.host' \
  "$preflight" > "$fixture/local-preflight"
mv "$fixture/local-preflight" "$preflight"
AURA_E2E_REMOTE='' AURA_E2E_LOCAL_PEER_ROOT="$peer_root" \
  AURA_E2E_LOCAL_PEER_RUN_DIR="$peer_run" AURA_E2E_RUN_DIR="$local_run" \
  PATH="$fixture/bin:$PATH" bash "$here/paired-enrollment.sh" \
  > "$fixture/local.stdout" 2> "$fixture/local.stderr"
local_result="$local_run/private-enrollment-$AURA_E2E_RUN_TOKEN/single-host-result.json"
jq -e '.phase=="single-host-result" and .cross_host_asserted==false and
  .process_identity.local.pid!=.process_identity.remote.pid and
  .transport.network_delivery_asserted==true and .assertions.exact_operation_and_host_ack' \
  "$local_result" >/dev/null
for secret in signed-setup-secret issued-code-secret signed-manifest-secret retained-verifier-secret; do
  ! rg -Fq "$secret" "$fixture/local.stdout" "$fixture/local.stderr" "$local_run"
done
# A one-host preflight cannot claim cross-host evidence or be reused as LAN.
before=$(wc -l < "$FIXTURE_LOG")
jq '.cross_host_asserted=true' "$preflight" > "$fixture/forged-local-preflight"
mv "$fixture/forged-local-preflight" "$preflight"
if AURA_E2E_REMOTE='' AURA_E2E_LOCAL_PEER_ROOT="$peer_root" \
    AURA_E2E_LOCAL_PEER_RUN_DIR="$peer_run" AURA_E2E_RUN_DIR="$fixture/forged-run" \
    PATH="$fixture/bin:$PATH" bash "$here/paired-enrollment.sh" \
    > "$fixture/forged-local.stdout" 2> "$fixture/forged-local.stderr"; then
  echo 'single-host diagnostic accepted a forged cross-host claim' >&2; exit 1
fi
[[ $(wc -l < "$FIXTURE_LOG") == "$before" ]]
unset AURA_E2E_LOCAL_PEER_ROOT AURA_E2E_LOCAL_PEER_RUN_DIR
jq '.phase="preflight" | .planned_process_identity.remote.host="fixture-remote" |
  .broker.policy="development_only_lan_http" |
  del(.cross_host_asserted)' "$preflight" > "$fixture/lan-preflight"
mv "$fixture/lan-preflight" "$preflight"
preflight="$AURA_E2E_ROOT/.tmp/lan-checklist-$AURA_E2E_RUN_TOKEN/paired-preflight.json"
before=$(wc -l < "$FIXTURE_LOG")
jq '.commit.remote="wrong-commit"' "$preflight" > "$fixture/wrong-preflight"
mv "$fixture/wrong-preflight" "$preflight"
if PATH="$fixture/bin:$PATH" bash "$here/paired-enrollment.sh" > "$fixture/preflight.stdout" 2> "$fixture/preflight.stderr"; then
  echo 'paired run accepted mismatched preflight' >&2; exit 1
fi
[[ $(wc -l < "$FIXTURE_LOG") == "$before" ]]
echo 'Paired LAN enrollment orchestration fixture passed'
