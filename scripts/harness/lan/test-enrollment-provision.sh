#!/usr/bin/env bash
# Isolated provision and private request fixtures; no LAN host is contacted.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/checkout/configs/harness" "$fixture/bin"
cp "$here/../../../configs/harness/lan-host-"{a,b}.toml "$fixture/checkout/configs/harness/"
export AURA_E2E_ROOT="$fixture/checkout" AURA_E2E_REMOTE=fixture-host
export AURA_E2E_REMOTE_ROOT='remote root' AURA_E2E_HOST_ADDR=192.0.2.10
export AURA_E2E_RUN_TOKEN=paired-enrollment-fixture
export FIXTURE_REMOTE_PARENT="$fixture" FIXTURE_SSH_ARGS="$fixture/ssh-args"
mkdir -p "$fixture/remote root"
cat > "$fixture/bin/ssh" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$FIXTURE_SSH_ARGS"
remote_command=${*: -1}
cd "$FIXTURE_REMOTE_PARENT"
bash -c "$remote_command"
SH
chmod +x "$fixture/bin/ssh"
if ! PATH="$fixture/bin:$PATH" bash "$here/provision-enrollment.sh" "$AURA_E2E_RUN_TOKEN" \
  > "$fixture/paths" 2> "$fixture/stderr"; then
  cat "$fixture/stderr" >&2
  exit 1
fi
local_config=$(sed -n 's/^AURA_E2E_LOCAL_CONFIG=//p' "$fixture/paths")
remote_config="$fixture/remote root/.tmp/e2e/enrollment-provision-$AURA_E2E_RUN_TOKEN/host-b.toml"
mode() {
  if stat -c %a "$1" >/dev/null 2>&1; then stat -c %a "$1";
  else stat -f %Lp "$1"; fi
}
[[ -f "$local_config" && -O "$local_config" && $(mode "$local_config") == 600 ]]
[[ -O "$(dirname "$local_config")" && $(mode "$(dirname "$local_config")") == 700 ]]
[[ -f "$remote_config" && -O "$remote_config" && $(mode "$remote_config") == 600 ]]
[[ -O "$(dirname "$remote_config")" && $(mode "$(dirname "$remote_config")") == 700 ]]
rg -q 'AURA_BOOTSTRAP_BROKER_BIND=192.0.2.10:43190' "$local_config"
rg -q 'AURA_BOOTSTRAP_BROKER_ALLOW_LAN_BIND=true' "$local_config"
rg -q 'AURA_BOOTSTRAP_BROKER_URL=http://192.0.2.10:43190' "$remote_config"
rg -q '^fixed_ports = true$' "$local_config"
rg -q '^fixed_ports = true$' "$remote_config"
[[ $(rg -c 'AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=' "$local_config") == 3 ]]
[[ $(rg -c 'AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=' "$remote_config") == 3 ]]
[[ $(rg -o 'AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=[0-9a-f]+' "$local_config" "$remote_config" | sed 's/^.*=//' | sort -u | wc -l | tr -d ' ') == 1 ]]
[[ $(rg -o 'AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN=[0-9a-f]+' "$local_config" "$remote_config" | sed 's/^.*=//' | sort -u | wc -l | tr -d ' ') == 6 ]]
! rg -q 'AURA_BOOTSTRAP_BROKER_AUTH_TOKEN|AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN' "$fixture/paths" "$fixture/stderr" "$fixture/ssh-args"
bearer=$(rg -o 'AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=[0-9a-f]+' "$local_config" | head -1)
! rg -Fq "${bearer#*=}" "$fixture/paths" "$fixture/stderr" "$fixture/ssh-args"
rg -q 'umask 077.*test ! -e.*chmod 600' "$fixture/ssh-args"
if PATH="$fixture/bin:$PATH" bash "$here/provision-enrollment.sh" "$AURA_E2E_RUN_TOKEN" \
  > "$fixture/duplicate-out" 2> "$fixture/duplicate-error"; then
  echo 'duplicate provision accepted' >&2; exit 1
fi
preexisting="$fixture/remote root/.tmp/e2e/enrollment-provision-preexisting-fixture"
mkdir -m 700 "$preexisting"
printf 'sentinel\n' > "$preexisting/keep"
if PATH="$fixture/bin:$PATH" bash "$here/provision-enrollment.sh" preexisting-fixture \
  > "$fixture/preexisting-out" 2> "$fixture/preexisting-error"; then
  echo 'preexisting remote destination accepted' >&2; exit 1
fi
[[ $(cat "$preexisting/keep") == sentinel && ! -e "$preexisting/host-b.toml" ]]
ln -s "$preexisting" "$fixture/remote root/.tmp/e2e/enrollment-provision-symlink-path-fixture"
if PATH="$fixture/bin:$PATH" bash "$here/provision-enrollment.sh" symlink-path-fixture \
  > "$fixture/symlink-out" 2> "$fixture/symlink-error"; then
  echo 'remote symlink destination accepted' >&2; exit 1
fi
[[ ! -e "$preexisting/host-b.toml" ]]
for invalid in missing-remote loopback-bind invalid-port; do
  status=0
  case "$invalid" in
    missing-remote) AURA_E2E_REMOTE= PATH="$fixture/bin:$PATH" bash "$here/provision-enrollment.sh" second-enrollment-fixture > "$fixture/out" 2> "$fixture/error" || status=$? ;;
    loopback-bind) AURA_E2E_HOST_ADDR=127.0.0.1 PATH="$fixture/bin:$PATH" bash "$here/provision-enrollment.sh" second-enrollment-fixture > "$fixture/out" 2> "$fixture/error" || status=$? ;;
    invalid-port) AURA_E2E_ENROLLMENT_BROKER_PORT=80 PATH="$fixture/bin:$PATH" bash "$here/provision-enrollment.sh" second-enrollment-fixture > "$fixture/out" 2> "$fixture/error" || status=$? ;;
  esac
  [[ "$status" != 0 ]]
  [[ ! -e "$fixture/checkout/.tmp/e2e/enrollment-provision-second-enrollment-fixture" ]]
done
echo 'LAN enrollment broker provision fixtures passed'
