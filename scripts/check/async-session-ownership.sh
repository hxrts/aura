#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "async-session-ownership: $*" >&2
  exit 1
}

cargo run -q -p aura-macros --bin ownership_lints -- \
  async-session-ownership \
  crates/aura-agent/src/handlers \
  crates/aura-agent/src/runtime/services \
  crates/aura-agent/src/runtime_bridge
