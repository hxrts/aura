#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-trace-determinism: $*" >&2
  exit 1
}

cargo test -p aura-harness repeated_runs_with_same_seed_share_same_report_shape --quiet \
  || fail "same-seed report/trace determinism test failed"

echo "harness trace determinism: clean"
