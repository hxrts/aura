#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-revision-contract: $*" >&2
  exit 1
}

rg -q 'pub revision: ProjectionRevision' crates/aura-app/src/ui_contract.rs \
  || fail "UiSnapshot must carry revision metadata"
rg -q 'pub quiescence: QuiescenceSnapshot' crates/aura-app/src/ui_contract.rs \
  || fail "UiSnapshot must carry quiescence metadata"

cargo test -p aura-app projection_revision_detects_stale_snapshots_by_revision --quiet

echo "harness revision contract: clean"
