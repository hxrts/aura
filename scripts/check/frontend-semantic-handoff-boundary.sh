#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo run -q -p aura-macros --bin ownership_lints -- \
  frontend-semantic-handoff-boundary \
  crates/aura-terminal \
  crates/aura-web
