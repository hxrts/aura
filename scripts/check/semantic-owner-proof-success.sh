#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

nix develop --command cargo run -q -p aura-macros --bin ownership_lints -- \
  semantic-owner-proof-success \
  crates/aura-app/src \
  crates/aura-terminal/src \
  crates/aura-web/src
