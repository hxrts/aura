#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-recovery-contract: $*" >&2
  exit 1
}

cargo test -p aura-harness registered_recoveries_cover_all_paths --quiet

rg -q 'pub const REGISTERED_RECOVERIES' crates/aura-harness/src/recovery_registry.rs \
  || fail "missing registered recovery metadata"

echo "harness recovery contract: clean"
