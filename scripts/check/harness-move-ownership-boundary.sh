#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-move-ownership-boundary: $*" >&2
  exit 1
}

cargo run -q -p aura-macros --bin ownership_lints -- \
  harness-move-ownership-boundary \
  crates/aura-app \
  crates/aura-terminal \
  crates/aura-web \
  crates/aura-harness
