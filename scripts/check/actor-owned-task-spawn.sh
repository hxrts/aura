#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo run -q -p aura-macros --bin ownership_lints -- \
  actor-owned-task-spawn \
  crates/aura-agent/src \
  crates/aura-app/src \
  crates/aura-core/src \
  crates/aura-effects/src \
  crates/aura-harness/src \
  crates/aura-terminal/src \
  crates/aura-ui/src \
  crates/aura-web/src
