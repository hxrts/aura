#!/usr/bin/env bash
# Harness-only, checklist-owned loopback forward to the authenticated LAN broker.
# The browser sees loopback HTTP; SSH protects the cross-host hop. No broker
# credential is passed to SSH or written by this script.
# Wrap batch.sh, not its inner checklist: the forward must be ready before
# fresh.sh starts the recipient browser.
# Usage: browser-broker-relay.sh <recipient-loopback-port> -- <batch> [args...]
set -euo pipefail
set +x
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$here/env.sh"
port=${1:-}
[[ "$port" =~ ^[0-9]+$ ]] && (( port > 1023 && port < 65536 )) &&
  [[ "${2:-}" == -- && $# -ge 3 ]] || {
  echo 'usage: browser-broker-relay.sh <loopback-port> -- <batch> [args...]' >&2
  exit 2
}
shift 2
[[ "$AURA_E2E_REMOTE" =~ ^[A-Za-z0-9._-]+@[A-Za-z0-9._-]+$ ]] || {
  echo 'browser broker relay requires a fixed SSH user and host' >&2; exit 2;
}
[[ "$AURA_E2E_HOST_ADDR" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  echo 'browser broker relay requires a private IPv4 broker host' >&2; exit 2;
}
IFS=. read -r a b c d <<< "$AURA_E2E_HOST_ADDR"
for octet in "$a" "$b" "$c" "$d"; do
  [[ "$octet" =~ ^(0|[1-9][0-9]{0,2})$ ]] && (( 10#$octet <= 255 )) || exit 2
done
(( a == 10 || (a == 172 && b >= 16 && b <= 31) || (a == 192 && b == 168) )) || {
  echo 'browser broker relay refuses a non-private broker host' >&2; exit 2;
}
broker_port=${AURA_E2E_ENROLLMENT_BROKER_PORT:-43190}
[[ "$broker_port" =~ ^[0-9]+$ ]] && (( broker_port > 1023 && broker_port < 65536 )) || exit 2

relay_pid=''
checklist_pid=''
cleanup() {
  local status=$?
  trap - EXIT INT TERM
  if [[ -n "$checklist_pid" ]]; then
    kill -TERM "$checklist_pid" 2>/dev/null || true
    wait "$checklist_pid" 2>/dev/null || true
  fi
  if [[ -n "$relay_pid" ]]; then
    kill -TERM "$relay_pid" 2>/dev/null || true
    wait "$relay_pid" 2>/dev/null || true
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Reverse forwarding is initiated by the already-authenticated issuer host.
# ExitOnForwardFailure refuses a reused/occupied recipient loopback port.
env -u AURA_HARNESS_BROWSER_BROKER_AUTH \
  -u AURA_BOOTSTRAP_BROKER_AUTH_TOKEN \
  -u AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN \
  ssh -o BatchMode=yes -o StrictHostKeyChecking=yes \
  -o ExitOnForwardFailure=yes -o ServerAliveInterval=5 -o ServerAliveCountMax=2 \
  -N -R "127.0.0.1:$port:$AURA_E2E_HOST_ADDR:$broker_port" \
  "$AURA_E2E_REMOTE" </dev/null >/dev/null 2>/dev/null &
relay_pid=$!

# Query the remote loopback listener, never the unauthenticated broker body.
# This is readiness for the SSH-owned port only; the paired checklist retains
# the exact authenticated broker registration/queue/ACK assertions.
ready=false
for _ in 1 2 3 4 5 6 7 8 9 10; do
  kill -0 "$relay_pid" 2>/dev/null || break
  if env -u AURA_HARNESS_BROWSER_BROKER_AUTH \
    -u AURA_BOOTSTRAP_BROKER_AUTH_TOKEN \
    -u AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN \
    ssh -o BatchMode=yes -o StrictHostKeyChecking=yes \
    -o ConnectTimeout=3 "$AURA_E2E_REMOTE" \
    "nc -z 127.0.0.1 $port" </dev/null >/dev/null 2>/dev/null; then
    ready=true
    break
  fi
  sleep 1
done
[[ "$ready" == true ]] || {
  echo 'browser broker relay did not bind recipient loopback' >&2; exit 1;
}

"$@" &
checklist_pid=$!
wait "$checklist_pid"
status=$?
checklist_pid=''
kill -0 "$relay_pid" 2>/dev/null || {
  echo 'browser broker relay exited before checklist completion' >&2; exit 1;
}
exit "$status"
