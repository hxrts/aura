#!/usr/bin/env bash
# Start a fresh two-host LAN harness run and bring up the standard cast:
# Alex and Carol (TUIs on this host) and Barbara (TUI on the remote host),
# with Alex linked to Barbara and Carol as contacts.
#
# Usage: AURA_E2E_REMOTE=user@host scripts/harness/lan/fresh.sh <run-token>
#
# Both hosts need the same commit built (scripts/harness/lan/build.sh) and the
# remote checkout at $AURA_E2E_REMOTE_ROOT. Each host renders its config with
# its own LAN address (AURA_E2E_HOST_ADDR, detected by default). Settings: see
# env.sh.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
[ -n "${1:-}" ] || { echo "usage: $0 <run-token>" >&2; exit 2; }
export AURA_E2E_RUN_TOKEN="$1"
# shellcheck source=lib.sh
. "$here/lib.sh"
[ -n "$AURA_E2E_REMOTE" ] || { echo 'set AURA_E2E_REMOTE to the other host (user@host)' >&2; exit 2; }

local_config="${AURA_E2E_LOCAL_CONFIG:-configs/harness/lan-host-a.toml}"
remote_config="${AURA_E2E_REMOTE_CONFIG:-configs/harness/lan-host-b.toml}"
remote_nix="${AURA_E2E_REMOTE_NIX:-/nix/var/nix/profiles/default/bin/nix}"

"$AURA_E2E_DRV" stop
printf -v remote_command 'cd %q && scripts/harness/lan/drv.sh stop && AURA_E2E_RUN_TOKEN=%q %q develop --command scripts/harness/lan/drv.sh start %q' \
  "$AURA_E2E_REMOTE_ROOT" "$AURA_E2E_RUN_TOKEN" "$remote_nix" "$remote_config"
ssh -o BatchMode=yes "$AURA_E2E_REMOTE" "$remote_command" | tail -1
(cd "$AURA_E2E_ROOT" && nix develop --command "$AURA_E2E_DRV" start "$local_config" | tail -1)

a=$(onboard alex-tui Alex); b=$(onboard barbara-tui Barbara); c=$(onboard alex-tui2 Carol)
echo "A=$a B=$b C=$c" | tee "$AURA_E2E_RUN_DIR/ids-$AURA_E2E_RUN_TOKEN"
link alex-tui barbara-tui
link alex-tui alex-tui2
contacts alex-tui; contacts barbara-tui; contacts alex-tui2
