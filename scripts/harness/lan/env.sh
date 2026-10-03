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
#   AURA_E2E_RUN_TOKEN    unique lowercase/hyphen run id; both hosts use the
#                         same token; at least 16 bytes; each start creates a
#                         retained run bundle
#   AURA_E2E_REMOTE       ssh destination of the other host (e.g. user@192.168.0.32)
#   AURA_E2E_REMOTE_ROOT  repo checkout on the other host (default: ~/projects/aura)
#   AURA_E2E_REMOTE_PREFIX  instance-id prefix owned by the other host (default: barbara-)
#   AURA_E2E_HOST_ADDR    this host's LAN address, substituted for __HOST_ADDR__
#                         in the configs (default: detected from the primary
#                         interface)

AURA_E2E_ROOT="${AURA_E2E_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)}"
AURA_E2E_RUN_DIR="${AURA_E2E_RUN_DIR:-$AURA_E2E_ROOT/.tmp/e2e/run}"
AURA_E2E_TOOL_REPL="${AURA_E2E_TOOL_REPL:-$AURA_E2E_ROOT/target/release/tool_repl}"
AURA_E2E_AURA_BIN="${AURA_E2E_AURA_BIN:-$AURA_E2E_ROOT/bin/aura}"
AURA_E2E_RUN_TOKEN="${AURA_E2E_RUN_TOKEN:-lan-e2e-shared-run}"
AURA_E2E_REMOTE="${AURA_E2E_REMOTE:-}"
AURA_E2E_REMOTE_ROOT="${AURA_E2E_REMOTE_ROOT:-projects/aura}"
AURA_E2E_REMOTE_PREFIX="${AURA_E2E_REMOTE_PREFIX:-barbara-}"
if [[ -z "${AURA_E2E_HOST_ADDR:-}" ]]; then
  AURA_E2E_HOST_ADDR="$( (ipconfig getifaddr en0 || ipconfig getifaddr en1 || hostname -I 2>/dev/null | awk '{print $1}') 2>/dev/null | head -1)"
fi
export AURA_E2E_ROOT AURA_E2E_RUN_DIR AURA_E2E_TOOL_REPL AURA_E2E_AURA_BIN \
  AURA_E2E_RUN_TOKEN AURA_E2E_REMOTE AURA_E2E_REMOTE_ROOT AURA_E2E_REMOTE_PREFIX \
  AURA_E2E_HOST_ADDR
