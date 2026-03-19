#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-recovery-ownership: $*" >&2
  exit 1
}

cargo run -q -p aura-macros --bin ownership_lints -- \
  harness-recovery-ownership \
  crates/aura-harness/src/tool_api.rs \
  crates/aura-terminal/src/tui/harness_state/snapshot.rs \
  crates/aura-ui/src/model.rs \
  crates/aura-web/src/harness_bridge.rs
