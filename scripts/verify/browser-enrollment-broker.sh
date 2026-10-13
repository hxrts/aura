#!/usr/bin/env bash
# Own a loopback broker for one browser enrollment harness command.
set -euo pipefail
umask 077

if (( $# == 0 )); then
  echo 'usage: AURA_BROWSER_BROKER_FIXTURE=PATH browser-enrollment-broker.sh COMMAND [ARGS...]' >&2
  exit 2
fi
: "${AURA_BROWSER_BROKER_FIXTURE:?build bootstrap_broker_fixture first}"
[[ -x "$AURA_BROWSER_BROKER_FIXTURE" ]] || { echo 'broker fixture is not executable' >&2; exit 2; }
command -v openssl >/dev/null || { echo 'openssl is required for broker authorization' >&2; exit 2; }
command -v jq >/dev/null || { echo 'jq is required for broker readiness' >&2; exit 2; }

export AURA_BROWSER_BROKER_AUTH_TOKEN
AURA_BROWSER_BROKER_AUTH_TOKEN="$(openssl rand -hex 32)"
broker_pid=''
control_port=''
readiness_dir="$(mktemp -d "${TMPDIR:-/tmp}/aura-browser-broker.XXXXXX")"
mkfifo "$readiness_dir/ready"
cleanup() {
  local status=$? acknowledgment='' cleanup_status=0
  trap - EXIT
  set +e
  if [[ -n "$control_port" ]]; then
    if exec 4<>"/dev/tcp/127.0.0.1/$control_port"; then
      printf 'STOP\n' >&4
      IFS= read -r -t 5 acknowledgment <&4
      [[ "$acknowledgment" == DISPOSED ]] || cleanup_status=1
      exec 4>&-
    else
      cleanup_status=1
    fi
  fi
  if [[ -n "$broker_pid" ]]; then
    if [[ -z "$control_port" || "$cleanup_status" -ne 0 ]]; then
      kill "$broker_pid" 2>/dev/null || true
    fi
    wait "$broker_pid" || cleanup_status=1
  fi
  exec 3>&- 2>/dev/null || true
  rm -rf "$readiness_dir"
  if (( status == 0 )); then status=$cleanup_status; fi
  exit "$status"
}
trap cleanup EXIT

exec 3<>"$readiness_dir/ready"
"$AURA_BROWSER_BROKER_FIXTURE" >"$readiness_dir/ready" &
broker_pid=$!
# The fixture emits one JSON readiness line on stdout. Keep its diagnostics on
# stderr; neither the authorization token nor per-page retrieval tokens are logged.
IFS= read -r -t 30 readiness <&3
control_port="$(jq -er '.control_port | select(type=="number" and .>0 and .<65536)' <<< "$readiness")"
export AURA_HARNESS_BROWSER_BROKER_URL AURA_HARNESS_BROWSER_BROKER_AUTH
AURA_HARNESS_BROWSER_BROKER_URL="$(jq -er '.broker_url | select(type=="string" and test("^http://127\\.0\\.0\\.1:[0-9]+$"))' <<< "$readiness")"
AURA_HARNESS_BROWSER_BROKER_AUTH="$AURA_BROWSER_BROKER_AUTH_TOKEN"
"$@"
