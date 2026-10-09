#!/usr/bin/env bash
# Fail-closed fresh-run sequencing, isolated from profiles/processes/network.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
fixture_root=$(mktemp -d)
trap 'rm -rf "$fixture_root"' EXIT
export FIXTURE_ROOT="$fixture_root"
mkdir -p "$fixture_root/driver" "$fixture_root/bin" "$fixture_root/local/scripts/harness/lan" "$fixture_root/remote root's/scripts/harness/lan"
cp "$here/fresh.sh" "$fixture_root/driver/fresh.sh"
cat > "$fixture_root/driver/lib.sh" <<'LIB'
export AURA_E2E_ROOT="$FIXTURE_ROOT/local"
export AURA_E2E_DRV="$AURA_E2E_ROOT/scripts/harness/lan/drv.sh"
export AURA_E2E_REMOTE=fixture-host
export AURA_E2E_REMOTE_ROOT="$FIXTURE_ROOT/remote root's"
export AURA_E2E_REMOTE_NIX="$FIXTURE_ROOT/bin/nix"
export AURA_E2E_REMOTE_CONFIG="remote config's.toml"
export AURA_E2E_LOCAL_CONFIG="local config's.toml"
export AURA_E2E_RUN_DIR="$FIXTURE_ROOT/evidence"
onboard() { echo "$1"; }
link() { :; }
contacts() { :; }
LIB
cat > "$fixture_root/bin/nix" <<'NIX'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == develop && "$2" == --command ]]
shift 2
exec "$@"
NIX
cat > "$fixture_root/bin/ssh" <<'SSH'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == -o && "$2" == BatchMode=yes && "$3" == fixture-host ]]
printf 'ssh\n' >> "$FIXTURE_ROOT/actions"
bash -euc "$4"
SSH
cat > "$fixture_root/driver/drv.sh" <<'DRV'
#!/usr/bin/env bash
set -euo pipefail
case "$PWD" in *"remote root's") host=remote;; *) host=local;; esac
printf '%s:%s\n' "$host" "$1" >> "$FIXTURE_ROOT/actions"
if [[ "$1" == stop && "$FAIL_HOST" == "$host" ]]; then
  echo "owned shutdown refused on $host; evidence retained" >&2
  exit 23
fi
if [[ "$1" == start ]]; then
  [[ "$2" == "$host config's.toml" ]]
  [[ "$AURA_E2E_RUN_TOKEN" == "run token's" ]]
  echo started
fi
DRV
cp "$fixture_root/driver/drv.sh" "$fixture_root/local/scripts/harness/lan/drv.sh"
cp "$fixture_root/driver/drv.sh" "$fixture_root/remote root's/scripts/harness/lan/drv.sh"
chmod +x "$fixture_root/bin/nix" "$fixture_root/bin/ssh" "$fixture_root/local/scripts/harness/lan/drv.sh" "$fixture_root/remote root's/scripts/harness/lan/drv.sh"
export PATH="$fixture_root/bin:$PATH"
# Pin fixture behavior rather than inherit any caller run/config/time policy.
unset AURA_E2E_LOCAL_CONFIG AURA_E2E_REMOTE_CONFIG AURA_E2E_REMOTE_NIX BASH_ENV
mkdir -p "$fixture_root/evidence"
printf 'retained evidence\n' > "$fixture_root/evidence/owned-state"
for host in local remote; do
  export FAIL_HOST="$host"
  : > "$fixture_root/actions"
  if bash "$fixture_root/driver/fresh.sh" "run token's" > "$fixture_root/out" 2> "$fixture_root/error"; then
    echo "$host stop failure incorrectly succeeded" >&2; exit 1
  fi
  [[ $(cat "$fixture_root/evidence/owned-state") == 'retained evidence' ]]
  rg -q "owned shutdown refused on $host" "$fixture_root/error"
  if rg -q ':start' "$fixture_root/actions"; then
    echo "$host stop failure allowed a start" >&2; exit 1
  fi
  if [[ "$host" == local ]] && rg -q '^ssh$' "$fixture_root/actions"; then
    echo 'local stop failure contacted remote host' >&2; exit 1
  fi
done
export FAIL_HOST=none
: > "$fixture_root/actions"
bash "$fixture_root/driver/fresh.sh" "run token's" > "$fixture_root/out"
expected=$'local:stop\nssh\nremote:stop\nremote:start\nlocal:start'
[[ $(cat "$fixture_root/actions") == "$expected" ]]
echo 'fresh-run shutdown sequencing fixtures passed'
