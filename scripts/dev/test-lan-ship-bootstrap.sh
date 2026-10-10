#!/usr/bin/env bash
# Exercise the actual ship stop/bootstrap command with isolated SSH/Nix facades.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
fixture=$(cd "$fixture" && pwd -P)
local_root="$fixture/local checkout"
remote_home="$fixture/remote-home"
mkdir -p "$remote_home"
case "${1:-absolute}" in
  absolute) remote_root="$fixture/remote checkout's"; remote_physical="$remote_root";;
  relative) remote_root="projects/aura"; remote_physical="$remote_home/$remote_root";;
  *) exit 2;;
esac
export FIXTURE_REMOTE_HOME="$remote_home"
mkdir -p "$local_root/scripts/harness/lan" "$remote_physical/scripts/harness/lan" "$fixture/bin" "$fixture/remote-bin"
cp "$repo_root/scripts/harness/lan/"{ship,drv,env}.sh "$local_root/scripts/harness/lan/"
cp "$repo_root/scripts/harness/lan/env.sh" "$remote_physical/scripts/harness/lan/env.sh"
printf '#!/usr/bin/env bash\nprintf obsolete > "$FIXTURE_OLD_DRIVER"\nexit 99\n' > "$remote_physical/scripts/harness/lan/drv.sh"
export FIXTURE_OLD_DRIVER="$fixture/old-driver-used" FIXTURE_COPY="$fixture/copied" FIXTURE_ROOT="$fixture/rooted"
cat > "$fixture/bin/git" <<'EOF'
#!/usr/bin/env bash
case "$1" in status) exit 0;; rev-parse) printf '%040d\n' 1;; *) exit 98;; esac
EOF
cat > "$fixture/bin/nix" <<'EOF'
#!/usr/bin/env bash
if [[ "$1" == develop ]]; then
  [[ "$3" == --command ]]
  shift 3
  export IN_NIX_SHELL=fixture
  exec "$@"
fi
[[ "$1" == copy ]]
printf copy > "$FIXTURE_COPY"

EOF
cat > "$fixture/bin/nix-store" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat > "$fixture/remote-bin/nix-store" <<'EOF'
#!/usr/bin/env bash
[[ -f "$FIXTURE_COPY" ]]
printf root > "$FIXTURE_ROOT"
EOF
cat > "$fixture/remote-bin/nix" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ -f "$FIXTURE_COPY" && -f "$FIXTURE_ROOT" ]]
[[ "$1" == develop && "$3" == --command ]]
shift 3
export IN_NIX_SHELL=fixture
exec "$@"
EOF
cat > "$fixture/bin/ssh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
shift 3
if [[ "$1" == 'uname -sm' ]]; then exec uname -sm; fi
unset AURA_E2E_ROOT AURA_E2E_RUN_DIR
cd "$FIXTURE_REMOTE_HOME"
exec bash -c "$1"
EOF
chmod +x "$fixture/bin/"* "$fixture/remote-bin/"*
export PATH="$fixture/bin:$PATH"
export AURA_E2E_ROOT="$local_root" AURA_E2E_REMOTE=fixture-host AURA_E2E_REMOTE_ROOT="$remote_root"
export AURA_NIX_BIN="$fixture/bin/nix" AURA_NIX_REMOTE_BIN="$fixture/remote-bin"
export AURA_E2E_HOST_ADDR=127.0.0.1
unset AURA_E2E_RUN_DIR IN_NIX_SHELL
status=0
bash "$local_root/scripts/harness/lan/ship.sh" --no-build > "$fixture/output" 2>&1 || status=$?
[[ "$status" == 1 ]]
grep -q 'ship: missing artifact' "$fixture/output"
[[ -f "$FIXTURE_COPY" && -f "$FIXTURE_ROOT" && ! -e "$FIXTURE_OLD_DRIVER" ]]
[[ -f "$local_root/.tmp/e2e/run.lifecycle.lock" && -f "$remote_physical/.tmp/e2e/run.lifecycle.lock" ]]
printf 'LAN ship portable-lock bootstrap fixture passed\n'
