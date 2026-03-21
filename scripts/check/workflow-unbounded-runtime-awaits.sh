#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

nix develop --command cargo run -q -p aura-macros --bin ownership_lints -- \
  workflow-unbounded-runtime-awaits \
  crates/aura-app/src \
  crates/aura-terminal/src/tui \
  crates/aura-web/src \
  crates/aura-ui/src
