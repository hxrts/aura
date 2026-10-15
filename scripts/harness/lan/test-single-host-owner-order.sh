#!/usr/bin/env bash
# Peer and issuer share one native binary, but each owns its own run lifetime.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d /private/tmp/aura-single-host-order.XXXXXX)
trap '[[ "${KEEP_FIXTURE:-0}" == 1 ]] || rm -rf "$fixture"' EXIT
local_root=$fixture/local
peer_root=$fixture/peer
mkdir -p "$local_root/scripts/harness/lan" "$local_root/configs/harness" \
  "$local_root/bin" "$local_root/target/release"
cp "${AURA_E2E_SINGLE_HOST_SCRIPT_FOR_TEST:-$here/single-host-enrollment.sh}" \
  "$local_root/scripts/harness/lan/single-host-enrollment.sh"
cat > "$local_root/.gitignore" <<'EOF'
.tmp/
EOF
cat > "$local_root/configs/harness/lan-host-a.toml" <<'EOF'
[run]
artifact_dir = ".tmp/e2e/run/host-a/artifacts"
EOF
cp "$local_root/configs/harness/lan-host-a.toml" \
  "$local_root/configs/harness/lan-host-b.toml"
cat > "$local_root/scripts/harness/lan/render-single-host-config.sh" <<'EOF'
render_single_host_config() { printf '[run]\n' > "$2"; }
EOF
cat > "$local_root/scripts/harness/lan/lib.sh" <<'EOF'
AURA_E2E_DRV=$here/drv.sh
onboard() { printf 'onboard:%s\n' "$1" >> "$FIXTURE_ORDER_LOG"; }
EOF
cat > "$local_root/scripts/harness/lan/paired-enrollment.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'approval\n' >> "$FIXTURE_ORDER_LOG"
EOF
cat > "$fixture/fake-aura.c" <<'C'
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/file.h>
#include <unistd.h>
int main(void) {
    int held = open(getenv("FIXTURE_SHARED_FILE"), O_CREAT | O_RDWR, 0600);
    if (held < 0 || flock(held, LOCK_SH) != 0) return 2;
    FILE *ready = fopen(getenv("FIXTURE_READY_FIFO"), "w");
    if (!ready) return 3;
    fputs("ready\n", ready);
    fclose(ready);
    FILE *release = fopen(getenv("FIXTURE_RELEASE_FIFO"), "r");
    if (!release) return 4;
    (void)fgetc(release);
    fclose(release);
    return 0;
}
C
cc -std=c11 -Wall -Wextra -Werror "$fixture/fake-aura.c" -o "$local_root/bin/aura"
cp "$local_root/bin/aura" "$local_root/target/release/tool_repl"
cat > "$local_root/scripts/harness/lan/drv.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
role=peer
[[ "$AURA_E2E_ROOT" == "$FIXTURE_LOCAL_ROOT" ]] && role=local
shared_root=$FIXTURE_LOCAL_ROOT/.tmp/e2e/run
mkdir -p "$shared_root"
live_shared_file() {
  local file
  for file in "$shared_root"/shared-*.open; do
    [[ -f "$file" ]] || continue
    if lsof -nP "$file" 2>/dev/null | rg -q 'aura'; then return 0; fi
  done
  return 1
}
case "$1" in
  start)
    if [[ "$role" == local ]]; then
      live_shared_file && { echo 'owned shared-binary process blocks local owner' >&2; exit 1; }
    else
      export FIXTURE_SHARED_FILE="$shared_root/shared-$AURA_E2E_RUN_TOKEN.open"
      "$AURA_E2E_AURA_BIN" >/dev/null 2>&1 &
      printf '%s\n' "$!" > "$FIXTURE_PEER_PID_FILE"
      IFS= read -r -t 5 ready < "$FIXTURE_READY_FIFO"
      [[ "$ready" == ready ]]
    fi
    printf '%s-start\n' "$role" >> "$FIXTURE_ORDER_LOG"
    ;;
  finalize)
    if [[ "$role" == peer ]]; then
      printf 'release\n' > "$FIXTURE_RELEASE_FIFO"
      # The shared descriptor is released by process exit, never by a timer.
      exec 8>>"$shared_root/shared-$AURA_E2E_RUN_TOKEN.open"
      flock -x -w 5 8
    else
      live_shared_file && { echo 'owned peer child still live at local finalization' >&2; exit 1; }
    fi
    printf '%s-finish\n' "$role" >> "$FIXTURE_ORDER_LOG"
    ;;
  *) exit 2 ;;
esac
EOF
chmod +x "$local_root/scripts/harness/lan/"{drv.sh,paired-enrollment.sh,single-host-enrollment.sh}
git -C "$local_root" init -q
git -C "$local_root" add .
git -C "$local_root" -c user.name=Fixture -c user.email=fixture@example.invalid \
  commit -qm 'fixed test source'
git clone -q "$local_root" "$peer_root"
export FIXTURE_LOCAL_ROOT=$local_root
export FIXTURE_ORDER_LOG=$fixture/order.log
export FIXTURE_PEER_PID_FILE=$fixture/peer.pid
export FIXTURE_READY_FIFO=$fixture/ready
export FIXTURE_RELEASE_FIFO=$fixture/release
mkfifo "$FIXTURE_READY_FIFO" "$FIXTURE_RELEASE_FIFO"
touch "$FIXTURE_ORDER_LOG"

# The positive path must start the issuer before the receiver, then dispose
# the receiver before the issuer. The receiver holds a shared binary and an
# open file in the issuer checkout throughout approval.
bash "$local_root/scripts/harness/lan/single-host-enrollment.sh" \
  driver-order-positive "$peer_root" > "$fixture/positive.out" 2> "$fixture/positive.err"
expected=$'local-start\npeer-start\nonboard:alex-tui\nonboard:barbara-tui\napproval\npeer-finish\nlocal-finish'
[[ "$(cat "$FIXTURE_ORDER_LOG")" == "$expected" ]]

# A prior, unregistered aura process with a different token remains a hard
# refusal before a new local owner starts, even with the positive ordering.
printf '' > "$FIXTURE_ORDER_LOG"
export FIXTURE_SHARED_FILE="$local_root/.tmp/e2e/run/shared-prior-orphan.open"
"$local_root/bin/aura" >/dev/null 2>&1 &
orphan_pid=$!
IFS= read -r -t 5 ready < "$FIXTURE_READY_FIFO"
[[ "$ready" == ready ]]
if bash "$local_root/scripts/harness/lan/single-host-enrollment.sh" \
    driver-order-negative "$peer_root" > "$fixture/negative.out" 2> "$fixture/negative.err"; then
  echo 'single-host driver admitted a prior open-file owner' >&2; exit 1
fi
rg -q 'owned shared-binary process blocks local owner' "$fixture/negative.err"
! rg -q 'peer-start|approval' "$FIXTURE_ORDER_LOG"
printf 'release\n' > "$FIXTURE_RELEASE_FIFO"
wait "$orphan_pid"
echo 'single-host owner ordering fixture passed'
