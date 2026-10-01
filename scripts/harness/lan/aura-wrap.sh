#!/usr/bin/env bash
# Launches the aura binary for a harness TUI instance and appends its stderr
# to a plaintext per-device log under $AURA_E2E_RUN_DIR/logs. Runtime tracing
# is also written under the harness transient root in harness mode.
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=env.sh
. "$here/env.sh"
mkdir -p "$AURA_E2E_RUN_DIR/logs"
name="unknown"; prev=""
for a in "$@"; do [ "$prev" = "--device-id" ] && name="$a"; prev="$a"; done
export AURA_TUI_ALLOW_STDIO=1
RL="$AURA_E2E_RUN_DIR/rust_log"; export RUST_LOG="${RUST_LOG:-$( [ -s "$RL" ] && cat "$RL" || echo info)}"
exec "$AURA_E2E_AURA_BIN" "$@" 2>>"$AURA_E2E_RUN_DIR/logs/$name.log"
