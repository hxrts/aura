#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

nix develop --command cargo run -q -p aura-macros --bin ownership_lints -- \
  workflow-no-view-derived-recipient-resolution \
  crates/aura-app/src/workflows
