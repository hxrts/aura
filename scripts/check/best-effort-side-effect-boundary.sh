#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

nix develop --command cargo run -q -p aura-macros --bin ownership_lints -- \
  best-effort-side-effect-boundary \
  crates
