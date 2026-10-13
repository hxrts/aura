#!/usr/bin/env bash
# Deterministic two-actor ToolApi fixture; no LAN host is contacted.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir -m 700 "$fixture/evidence"
mkdir "$fixture/bin"
export AURA_E2E_ROOT="$fixture/checkout"
mkdir -p "$AURA_E2E_ROOT/.tmp/lan-checklist-paired-enrollment-fixture"
export AURA_E2E_RUN_DIR="$fixture/evidence"
export AURA_E2E_RUN_TOKEN=paired-enrollment-fixture
export AURA_E2E_REMOTE=fixture-host AURA_E2E_REMOTE_ROOT=fixture-root
export AURA_E2E_HOST_ADDR=192.0.2.10 AURA_E2E_BOOTSTRAP=paired-enrollment
export AURA_E2E_DRV="$fixture/bin/driver" FIXTURE_LOG="$fixture/requests"
export AURA_E2E_AURA_BIN="$fixture/bin/aura" AURA_E2E_TOOL_REPL="$fixture/bin/tool_repl"
export AURA_E2E_WEB_INDEX="$fixture/web-index.html"
export FIXTURE_COMMIT=$(git -C "$here/../../.." rev-parse HEAD)
printf '{"phase":"preflight","commit":{"local":"%s","remote":"%s"},"run_token":"%s","network_delivery_asserted":false}\n' \
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
    echo '{"status":"ok","payload":{"screen":"neighborhood","readiness":"ready","operations":[],"runtime_events":[]}}' ;;
  barbara-tui:submit_semantic_command:ImportDeviceEnrollmentCode)
    jq -e '.params.intent.ImportDeviceEnrollmentCode=={code:"issued-code-secret",manifest_transfer:{manifest_code:"signed-manifest-secret",initiator_verifier_code:"retained-verifier-secret"}}' <<< "$request" >/dev/null || exit 1
    echo '{"status":"ok","payload":{"submission":"accepted","handle":{"ui_operation":{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2"}}}}' ;;
  barbara-tui:wait_for_ui_snapshot_event:)
    if [[ "${FIXTURE_CASE:-}" == missing_ack ]]; then
      echo '{"status":"ok","payload":{"event":{"version":3,"snapshot":{"screen":"neighborhood","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2","state":"succeeded","failure":null}],"runtime_events":[],"lists":[{"id":"devices","items":[]}]}}}}'
      exit 0
    fi
    if [[ "${FIXTURE_CASE:-}" == wrong_reopen_device && -e "$FIXTURE_RESTARTED" ]]; then
      echo '{"status":"ok","payload":{"event":{"version":4,"snapshot":{"screen":"neighborhood","readiness":"ready","operations":[],"runtime_events":[],"lists":[{"id":"devices","items":[{"id":"00000000-0000-4000-8000-000000000001"},{"id":"00000000-0000-4000-8000-000000000003"}]}]}}}}'
      exit 0
    fi
    echo '{"status":"ok","payload":{"event":{"version":3,"snapshot":{"screen":"neighborhood","readiness":"ready","operations":[{"id":"device_enrollment","instance_id":"tui-op-device_enrollment-2","state":"succeeded","failure":null}],"runtime_events":[{"id":"tui-runtime-fact-accepted","fact":{"kind":"device_enrollment_accepted","device_id":"00000000-0000-4000-8000-000000000002","device_count":2}}],"lists":[{"id":"devices","items":[{"id":"00000000-0000-4000-8000-000000000001"},{"id":"00000000-0000-4000-8000-000000000002"}]}]}}}}' ;;
  barbara-tui:get_authority_id:)
    if [[ "${FIXTURE_CASE:-}" == wrong_authority ]]; then
      echo '{"status":"ok","payload":{"authority_id":"other-authority","source":"backend"}}'
      exit 0
    fi
    echo '{"status":"ok","payload":{"authority_id":"authority-shared","source":"backend"}}' ;;
  alex-tui:get_authority_id:)
    echo '{"status":"ok","payload":{"authority_id":"authority-shared","source":"backend"}}' ;;
  barbara-tui:restart:)
    jq -e '.params.expected_operation_instance==null' <<< "$request" >/dev/null || exit 1
    : > "$FIXTURE_RESTARTED"
    echo '{"status":"ok","payload":{"status":"restarted"}}' ;;
  *) echo '{"status":"error","message":"unexpected fixture request"}' ;;
esac
DRIVER
chmod +x "$fixture/bin/ssh" "$fixture/bin/driver" "$fixture/bin/git"
export FIXTURE_RESTARTED="$fixture/restarted"
PATH="$fixture/bin:$PATH" bash "$here/paired-enrollment.sh" > "$fixture/stdout" 2> "$fixture/stderr"
manifest="$AURA_E2E_RUN_DIR/private-enrollment-$AURA_E2E_RUN_TOKEN/paired-result.json"
jq -e '.assertions.exact_operation_and_host_ack and .assertions.same_device_after_reopen and .transport.network_delivery_asserted==false' "$manifest" >/dev/null
for secret in signed-setup-secret issued-code-secret signed-manifest-secret retained-verifier-secret; do
  ! rg -Fq "$secret" "$fixture/stdout" "$fixture/stderr" "$AURA_E2E_RUN_DIR"
  rg -Fq "$secret" "$FIXTURE_LOG"
done
for failure in missing_ack wrong_authority wrong_reopen_device; do
  rm -f "$FIXTURE_RESTARTED"
  bad_dir="$fixture/$failure"
  mkdir -m 700 "$bad_dir"
  printf '{"pid":100,"birth":"fixture-local","run_token":"%s"}\n' "$AURA_E2E_RUN_TOKEN" > "$bad_dir/repl.identity.json"
  if FIXTURE_CASE="$failure" AURA_E2E_RUN_DIR="$bad_dir" PATH="$fixture/bin:$PATH" \
      bash "$here/paired-enrollment.sh" > "$fixture/$failure.stdout" 2> "$fixture/$failure.stderr"; then
    echo "paired fixture accepted $failure" >&2; exit 1
  fi
  [[ ! -e "$bad_dir/private-enrollment-$AURA_E2E_RUN_TOKEN/paired-result.json" ]]
  for secret in signed-setup-secret issued-code-secret signed-manifest-secret retained-verifier-secret; do
    ! rg -Fq "$secret" "$fixture/$failure.stdout" "$fixture/$failure.stderr" "$bad_dir"
  done
done
preflight="$AURA_E2E_ROOT/.tmp/lan-checklist-$AURA_E2E_RUN_TOKEN/paired-preflight.json"
before=$(wc -l < "$FIXTURE_LOG")
jq '.commit.remote="wrong-commit"' "$preflight" > "$fixture/wrong-preflight"
mv "$fixture/wrong-preflight" "$preflight"
if PATH="$fixture/bin:$PATH" bash "$here/paired-enrollment.sh" > "$fixture/preflight.stdout" 2> "$fixture/preflight.stderr"; then
  echo 'paired run accepted mismatched preflight' >&2; exit 1
fi
[[ $(wc -l < "$FIXTURE_LOG") == "$before" ]]
echo 'Paired LAN enrollment orchestration fixture passed'
