#!/usr/bin/env bash
# Provision an explicit development-only LAN broker for a fixed-commit paired
# enrollment batch. Credentials live only in owner-private generated configs.
# Usage: AURA_E2E_REMOTE=user@host provision-enrollment.sh <fresh-run-token>
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$here/env.sh"
token=${1:-}
[[ "$token" =~ ^[a-z0-9][a-z0-9-]*$ && ${#token} -ge 16 ]] || {
  echo 'provide a fresh LAN run token of at least 16 lowercase bytes' >&2; exit 2;
}
[[ -n "$AURA_E2E_REMOTE" && "$AURA_E2E_HOST_ADDR" != 127.* &&
   "$AURA_E2E_HOST_ADDR" != localhost && -n "$AURA_E2E_HOST_ADDR" ]] || {
  echo 'paired enrollment needs a remote host and a LAN-visible Host A address' >&2; exit 2;
}
command -v openssl >/dev/null || { echo 'openssl is required for broker credentials' >&2; exit 127; }
umask 077
port=${AURA_E2E_ENROLLMENT_BROKER_PORT:-43190}
[[ "$port" =~ ^[0-9]+$ ]] && (( port > 1023 && port < 65536 )) || {
  echo 'invalid enrollment broker port' >&2; exit 2;
}
broker_url="http://$AURA_E2E_HOST_ADDR:$port"
root="$AURA_E2E_ROOT/.tmp/e2e/enrollment-provision-$token"
[[ ! -L "$AURA_E2E_ROOT/.tmp" && ! -L "$AURA_E2E_ROOT/.tmp/e2e" ]] || {
  echo 'enrollment provision parent is a symlink' >&2; exit 1;
}
[[ ! -e "$root" && ! -L "$root" ]] || {
  echo 'enrollment provision already exists; use a fresh token' >&2; exit 1;
}
mkdir -p "$(dirname "$root")"
mkdir -m 700 "$root"
broker_auth=$(openssl rand -hex 32)
[[ "$broker_auth" =~ ^[0-9a-f]{64}$ ]] || exit 1

render() { # source config, output config, host | client
  local source_config=$1 output=$2 role=$3 line instance='' invitation=''
  : > "$output"
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == '[[instances]]' ]]; then
      instance=''
      invitation=$(openssl rand -hex 32)
      [[ "$invitation" =~ ^[0-9a-f]{64}$ ]] || return 1
    elif [[ "$line" =~ ^id[[:space:]]*=[[:space:]]*\"([^\"]+)\"$ ]]; then
      instance=${BASH_REMATCH[1]}
    fi
    printf '%s\n' "$line" >> "$output"
    if [[ "$line" == '[run]' ]]; then
      printf 'fixed_ports = true\n' >> "$output"
    fi
    if [[ "$line" =~ ^[[:space:]]*env[[:space:]]*=[[:space:]]*\[$ ]]; then
      [[ -n "$instance" && -n "$invitation" ]] || {
        echo 'instance environment appears before its identity' >&2; return 1;
      }
      printf '  "AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=%s",\n' "$broker_auth" >> "$output"
      printf '  "AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN=%s",\n' "$invitation" >> "$output"
      if [[ "$role" == host && "$instance" == alex-tui ]]; then
        printf '  "AURA_BOOTSTRAP_BROKER_BIND=%s:%s",\n' "$AURA_E2E_HOST_ADDR" "$port" >> "$output"
        printf '  "AURA_BOOTSTRAP_BROKER_ALLOW_LAN_BIND=true",\n' >> "$output"
      else
        printf '  "AURA_BOOTSTRAP_BROKER_URL=%s",\n' "$broker_url" >> "$output"
        printf '  "AURA_BOOTSTRAP_BROKER_ALLOW_PRIVATE_LAN_HTTP=true",\n' >> "$output"
      fi
    fi
  done < "$source_config"
  chmod 600 "$output"
}

render "$AURA_E2E_ROOT/configs/harness/lan-host-a.toml" "$root/host-a.toml" host
render "$AURA_E2E_ROOT/configs/harness/lan-host-b.toml" "$root/host-b.toml" client
remote_relative=".tmp/e2e/enrollment-provision-$token/host-b.toml"
printf -v remote_root '%q' "$AURA_E2E_REMOTE_ROOT"
printf -v remote_dir '%q' ".tmp/e2e/enrollment-provision-$token"
ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
  "cd $remote_root && umask 077 && test ! -L .tmp && test ! -L .tmp/e2e && mkdir -p .tmp/e2e && test ! -e $remote_dir && test ! -L $remote_dir && mkdir -m 700 $remote_dir && cat > $remote_dir/host-b.toml && chmod 600 $remote_dir/host-b.toml" \
  < "$root/host-b.toml"
printf 'AURA_E2E_LOCAL_CONFIG=%s\nAURA_E2E_REMOTE_CONFIG=%s\n' \
  "$root/host-a.toml" "$remote_relative"
echo 'paired enrollment broker provisioned (development-only LAN HTTP)' >&2
