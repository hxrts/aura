#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

nix develop --command cargo run -q -p aura-macros --bin ownership_lints -- \
  semantic-owner-detached-continuation \
  crates/aura-app/src \
  crates/aura-terminal/src \
  crates/aura-web/src \
  crates/aura-harness/src
