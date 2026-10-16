#!/usr/bin/env bash
# Isolated relay ownership and refusal fixture; never contacts a LAN host.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/bin"
export AURA_E2E_REMOTE=fixture@host AURA_E2E_HOST_ADDR=192.168.1.8
export FIXTURE_RELAY_ROOT="$fixture"
export AURA_HARNESS_BROWSER_BROKER_AUTH=fixture-secret-must-stay-out-of-ssh-args
export AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=fixture-bearer-must-stay-out-of-ssh-env
export AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN=fixture-invitation-must-stay-out-of-ssh-env
cat > "$fixture/bin/ssh" <<'SH'
#!/usr/bin/env bash
[[ -z "${AURA_HARNESS_BROWSER_BROKER_AUTH:-}" &&
   -z "${AURA_BOOTSTRAP_BROKER_AUTH_TOKEN:-}" &&
   -z "${AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN:-}" ]] || exit 3
printf '%s\n' "$*" >> "$FIXTURE_RELAY_ROOT/ssh-args"
case " $* " in
  *' -N -R '*)
    [[ "${FIXTURE_FAIL_FORWARD:-0}" != 1 ]] || exit 1
    : > "$FIXTURE_RELAY_ROOT/active"
    trap 'rm -f "$FIXTURE_RELAY_ROOT/active"; exit 0' TERM INT
    while :; do sleep 1; done ;;
  *' nc -z 127.0.0.1 43191 '*) [[ -e "$FIXTURE_RELAY_ROOT/active" ]] ;;
  *) exit 2 ;;
esac
SH
cat > "$fixture/bin/checklist" <<'SH'
#!/usr/bin/env bash
[[ -e "$FIXTURE_RELAY_ROOT/active" ]] || exit 1
[[ "${FIXTURE_FAIL_BATCH:-0}" != 1 ]] || exit 17
printf 'done\n' >> "$FIXTURE_RELAY_ROOT/checklist"
SH
chmod +x "$fixture/bin/ssh" "$fixture/bin/checklist"
PATH="$fixture/bin:$PATH" bash "$here/browser-broker-relay.sh" 43191 -- "$fixture/bin/checklist"
[[ -e "$fixture/checklist" && ! -e "$fixture/active" ]]
rg -q -- '-o ExitOnForwardFailure=yes.*-N -R 127.0.0.1:43191:192.168.1.8:43190 fixture@host' "$fixture/ssh-args"
! rg -qi 'bearer|credential|invitation|authorization|secret' "$fixture/ssh-args"
! rg -Fq "$AURA_HARNESS_BROWSER_BROKER_AUTH" "$fixture/ssh-args"
for bad in '127.0.0.1' '8.8.8.8' '192.168.1.999' '010.0.0.1'; do
  status=0
  AURA_E2E_HOST_ADDR="$bad" PATH="$fixture/bin:$PATH" \
    bash "$here/browser-broker-relay.sh" 43191 -- "$fixture/bin/checklist" \
    > "$fixture/out" 2> "$fixture/err" || status=$?
  [[ "$status" != 0 && ! -e "$fixture/active" ]]
done
status=0
AURA_E2E_REMOTE='' PATH="$fixture/bin:$PATH" \
  bash "$here/browser-broker-relay.sh" 43191 -- "$fixture/bin/checklist" \
  > "$fixture/out" 2> "$fixture/err" || status=$?
[[ "$status" != 0 && ! -e "$fixture/active" ]]
status=0
FIXTURE_FAIL_FORWARD=1 PATH="$fixture/bin:$PATH" \
  bash "$here/browser-broker-relay.sh" 43191 -- "$fixture/bin/checklist" \
  > "$fixture/out" 2> "$fixture/err" || status=$?
[[ "$status" != 0 && ! -e "$fixture/active" ]]
[[ $(cat "$fixture/checklist") == done ]]
status=0
FIXTURE_FAIL_BATCH=1 PATH="$fixture/bin:$PATH" \
  bash "$here/browser-broker-relay.sh" 43191 -- "$fixture/bin/checklist" \
  > "$fixture/out" 2> "$fixture/err" || status=$?
[[ "$status" == 17 && ! -e "$fixture/active" ]]
echo 'browser broker relay fixtures passed'
