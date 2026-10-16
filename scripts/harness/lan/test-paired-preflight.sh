#!/usr/bin/env bash
# Fixed-commit admission without contacting a host or starting a profile.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
export AURA_E2E_ROOT="$fixture/checkout" AURA_E2E_RUN_TOKEN=paired-preflight-fixture
export AURA_E2E_REMOTE=fixture-host AURA_E2E_REMOTE_ROOT="$fixture/remote root's"
export AURA_E2E_HOST_ADDR=192.0.2.10 AURA_E2E_BOOTSTRAP=paired-enrollment
export AURA_E2E_AURA_BIN="$fixture/aura" AURA_E2E_TOOL_REPL="$fixture/tool_repl"
export AURA_E2E_WEB_INDEX="$fixture/index.html" FIXTURE_COMMIT=fixture-commit
export AURA_E2E_LOCAL_CONFIG="$AURA_E2E_ROOT/.tmp/e2e/enrollment-provision-$AURA_E2E_RUN_TOKEN/host-a.toml"
export AURA_E2E_REMOTE_CONFIG=".tmp/e2e/enrollment-provision-$AURA_E2E_RUN_TOKEN/host-b.toml"
mkdir -p "$fixture/bin" "$AURA_E2E_ROOT/.tmp/main-smoke-$FIXTURE_COMMIT" "$(dirname "$AURA_E2E_LOCAL_CONFIG")"
mkdir -p "$AURA_E2E_ROOT/scripts/ci"
cat > "$AURA_E2E_ROOT/scripts/ci/main-smoke.sh" <<'MOCK'
#!/usr/bin/env bash
[[ "$1" == --verify && "$(cat .tmp/main-smoke-$FIXTURE_COMMIT/validated-commit)" == "$FIXTURE_COMMIT" ]]
MOCK
mkdir -p "$AURA_E2E_REMOTE_ROOT/$(dirname "$AURA_E2E_REMOTE_CONFIG")"
mkdir -m 700 "$fixture/evidence"
printf '%s\n' "$FIXTURE_COMMIT" > "$AURA_E2E_ROOT/.tmp/main-smoke-$FIXTURE_COMMIT/validated-commit"
printf 'aura\n' > "$AURA_E2E_AURA_BIN"
printf 'repl\n' > "$AURA_E2E_TOOL_REPL"
printf 'web\n' > "$AURA_E2E_WEB_INDEX"
cat > "$AURA_E2E_LOCAL_CONFIG" <<'CONFIG'
  "AURA_BOOTSTRAP_BROKER_BIND=192.0.2.10:43190",
  "AURA_BOOTSTRAP_BROKER_ALLOW_LAN_BIND=true",
  "AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=must-never-be-retained",
CONFIG
chmod 600 "$AURA_E2E_LOCAL_CONFIG"
cat > "$AURA_E2E_REMOTE_ROOT/$AURA_E2E_REMOTE_CONFIG" <<'CONFIG'
  "AURA_BOOTSTRAP_BROKER_URL=http://192.0.2.10:43190",
  "AURA_BOOTSTRAP_BROKER_URL=http://192.0.2.10:43190",
CONFIG
chmod 600 "$AURA_E2E_REMOTE_ROOT/$AURA_E2E_REMOTE_CONFIG"
export FIXTURE_AURA_HASH=$(sha256sum "$AURA_E2E_AURA_BIN" | cut -d' ' -f1)
export FIXTURE_REPL_HASH=$(sha256sum "$AURA_E2E_TOOL_REPL" | cut -d' ' -f1)
export FIXTURE_WEB_HASH=$(sha256sum "$AURA_E2E_WEB_INDEX" | cut -d' ' -f1)
cat > "$fixture/bin/git" <<'GIT'
#!/usr/bin/env bash
printf '%s\n' "$FIXTURE_COMMIT"
GIT
cat > "$fixture/bin/ssh" <<'SSH'
#!/usr/bin/env bash
case "$*" in
  *"stat -c '%a'"*) printf '600\n';;
  *'git rev-parse HEAD'*) printf '%s\n' "${FIXTURE_REMOTE_COMMIT:-$FIXTURE_COMMIT}";;
  *'sha256sum bin/aura'*)
    printf '%s  bin/aura\n%s  target/release/tool_repl\n%s  target/dx/aura-web/release/web/public/index.html\n' \
      "${FIXTURE_REMOTE_AURA_HASH:-$FIXTURE_AURA_HASH}" "$FIXTURE_REPL_HASH" "$FIXTURE_WEB_HASH";;
  *'AURA_HARNESS_BROWSER_BROKER_AUTH='*)
    [[ "$*" == *' develop --command '* ]] || exit 24
    remote_command=${*: -1}
    remote_command=${remote_command#* develop --command }
    (cd "$AURA_E2E_REMOTE_ROOT" && bash -c "$remote_command");;
  *'AURA_BOOTSTRAP_BROKER_URL'*|*'AURA_HARNESS_BROWSER_BROKER_'*) bash -c "${@: -1}";;
  *hostname*) printf 'fixture-remote\n';;
  *) exit 2;;
esac
SSH
chmod +x "$fixture/bin/git" "$fixture/bin/ssh"
manifest="$fixture/evidence/paired-preflight.json"
PATH="$fixture/bin:$PATH" bash "$here/paired-preflight.sh" "$manifest" > "$fixture/stdout"
jq -e '.phase=="preflight" and .commit.local=="fixture-commit" and
  .commit.remote=="fixture-commit" and
  .broker=={endpoint:"http://192.0.2.10:43190",policy:"development_only_lan_http",recipient_kind:"native"} and
  .local_smoke_receipt.validated_commit=="fixture-commit" and
  .artifact_sha256.local==.artifact_sha256.remote and
  .planned_process_identity.local.pid_birth=="pending_start" and
  .network_delivery_asserted==false' "$manifest" >/dev/null
[[ $(stat -c '%a' "$manifest" 2>/dev/null || stat -f '%Lp' "$manifest") == 600 ]]
! rg -Fq must-never-be-retained "$manifest" "$fixture/stdout"

for failure in commit hash broker; do
  rm -f "$manifest"
  case "$failure" in
    commit) FIXTURE_REMOTE_COMMIT=wrong-commit PATH="$fixture/bin:$PATH" bash "$here/paired-preflight.sh" "$manifest" > "$fixture/out" 2>&1 && exit 1;;
    hash) FIXTURE_REMOTE_AURA_HASH=$(printf '0%.0s' {1..64}) PATH="$fixture/bin:$PATH" bash "$here/paired-preflight.sh" "$manifest" > "$fixture/out" 2>&1 && exit 1;;
    broker) sed -i.bak 's/192\.0\.2\.10:43190/192.0.2.11:43190/' "$AURA_E2E_LOCAL_CONFIG"; PATH="$fixture/bin:$PATH" bash "$here/paired-preflight.sh" "$manifest" > "$fixture/out" 2>&1 && exit 1;;
  esac
  [[ ! -e "$manifest" ]]
done
sed -i.bak 's/192\.0\.2\.11:43190/192.0.2.10:43190/' "$AURA_E2E_LOCAL_CONFIG"
browser_auth=$(printf 'a%.0s' {1..64})
sed -i.bak "s/must-never-be-retained/$browser_auth/" "$AURA_E2E_LOCAL_CONFIG"
cat > "$AURA_E2E_REMOTE_ROOT/$AURA_E2E_REMOTE_CONFIG" <<CONFIG
  "AURA_HARNESS_BROWSER_BROKER_URL=http://127.0.0.1:43191",
  "AURA_HARNESS_BROWSER_BROKER_AUTH=$browser_auth",
CONFIG
AURA_E2E_ENROLLMENT_RECIPIENT=browser PATH="$fixture/bin:$PATH" \
  bash "$here/paired-preflight.sh" "$manifest" > "$fixture/browser-stdout"
jq -e '.broker.recipient_kind=="browser" and
  .planned_process_identity.remote.actor=="barbara-web" and
  .network_delivery_asserted==false' "$manifest" >/dev/null
! rg -Fq "$browser_auth" "$manifest" "$fixture/browser-stdout"
rm -f "$manifest"
wrong_browser_auth=$(printf 'b%.0s' {1..64})
sed -i.bak "s/$browser_auth/$wrong_browser_auth/" "$AURA_E2E_REMOTE_ROOT/$AURA_E2E_REMOTE_CONFIG"
if AURA_E2E_ENROLLMENT_RECIPIENT=browser PATH="$fixture/bin:$PATH" \
  bash "$here/paired-preflight.sh" "$manifest" > "$fixture/out" 2> "$fixture/err"; then
  echo 'browser preflight accepted a foreign broker bearer' >&2; exit 1
fi
[[ ! -e "$manifest" ]]
sed -i.bak "s/$wrong_browser_auth/$browser_auth/" "$AURA_E2E_REMOTE_ROOT/$AURA_E2E_REMOTE_CONFIG"
sed -i.bak 's/127\.0\.0\.1:43191/192.0.2.10:43191/' "$AURA_E2E_REMOTE_ROOT/$AURA_E2E_REMOTE_CONFIG"
if AURA_E2E_ENROLLMENT_RECIPIENT=browser PATH="$fixture/bin:$PATH" \
  bash "$here/paired-preflight.sh" "$manifest" > "$fixture/out" 2>&1; then
  echo 'browser preflight accepted non-loopback browser endpoint' >&2; exit 1
fi
[[ ! -e "$manifest" ]]
echo 'Paired LAN fixed-commit preflight fixtures passed'
