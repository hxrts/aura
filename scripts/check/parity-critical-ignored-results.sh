#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

nix develop --command cargo run -q -p aura-macros --bin ownership_lints -- \
  parity-critical-ignored-results \
  crates/aura-app/src/workflows \
  crates/aura-agent/src/handlers
