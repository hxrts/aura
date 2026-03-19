#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo run -q -p aura-macros --bin ownership_lints -- \
  timeout-policy-boundary \
  crates/aura-app/src/workflows \
  crates/aura-agent/src/handlers/invitation \
  crates/aura-agent/src/runtime_bridge \
  crates/aura-agent/src/runtime/effects \
  crates/aura-terminal/src/tui \
  crates/aura-harness/src
