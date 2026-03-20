#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-readiness-ownership: $*" >&2
  exit 1
}

cargo run -q -p aura-macros --bin ownership_lints -- \
  harness-readiness-ownership \
  crates/aura-agent/src/reactive/app_signal_views.rs \
  crates/aura-terminal/src \
  crates/aura-web/src \
  crates/aura-harness/src
