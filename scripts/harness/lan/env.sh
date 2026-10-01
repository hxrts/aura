# Shared settings for the multi-host (LAN) harness helpers. Sourced by the
# other scripts in this directory; every value can be overridden from the
# environment.
#
#   AURA_E2E_ROOT         repo checkout on this host (default: this repo)
#   AURA_E2E_RUN_DIR      REPL FIFO/output, logs and per-run state; must match
#                         the data_dir/artifact_dir prefix in the configs
#                         (default: .tmp/e2e/run, as in configs/harness/lan-host-*.toml)
#   AURA_E2E_TOOL_REPL    tool_repl binary (default: target/release/tool_repl)
#   AURA_E2E_AURA_BIN     aura binary launched for TUI instances (default: bin/aura)
#   AURA_E2E_RUN_TOKEN    shared harness run token; both hosts must use the same one
#   AURA_E2E_REMOTE       ssh destination of the other host (e.g. user@192.168.0.32)
#   AURA_E2E_REMOTE_ROOT  repo checkout on the other host (default: ~/projects/aura)
#   AURA_E2E_REMOTE_PREFIX  instance-id prefix owned by the other host (default: barbara-)

AURA_E2E_ROOT="${AURA_E2E_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)}"
AURA_E2E_RUN_DIR="${AURA_E2E_RUN_DIR:-$AURA_E2E_ROOT/.tmp/e2e/run}"
AURA_E2E_TOOL_REPL="${AURA_E2E_TOOL_REPL:-$AURA_E2E_ROOT/target/release/tool_repl}"
AURA_E2E_AURA_BIN="${AURA_E2E_AURA_BIN:-$AURA_E2E_ROOT/bin/aura}"
AURA_E2E_RUN_TOKEN="${AURA_E2E_RUN_TOKEN:-lan-e2e-shared-run}"
AURA_E2E_REMOTE="${AURA_E2E_REMOTE:-}"
AURA_E2E_REMOTE_ROOT="${AURA_E2E_REMOTE_ROOT:-projects/aura}"
AURA_E2E_REMOTE_PREFIX="${AURA_E2E_REMOTE_PREFIX:-barbara-}"
export AURA_E2E_ROOT AURA_E2E_RUN_DIR AURA_E2E_TOOL_REPL AURA_E2E_AURA_BIN \
  AURA_E2E_RUN_TOKEN AURA_E2E_REMOTE AURA_E2E_REMOTE_ROOT AURA_E2E_REMOTE_PREFIX
