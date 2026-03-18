#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

nix develop --command cargo run -q -p aura-macros --bin ownership_lints -- \
  semantic-owner-bounded-awaits \
  crates/aura-app/src/workflows
