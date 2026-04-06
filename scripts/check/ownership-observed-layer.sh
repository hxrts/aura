#!/usr/bin/env bash
# Enforce observed-layer authorship boundaries via compile-fail and lint checks.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "observed-layer-authorship: $*" >&2
  exit 1
}

# Composition script that delegates to:
#   1. aura-app compile-fail ownership boundaries — lifecycle/readiness publication boundaries
#   2. ownership_lints harness-readiness-ownership — readiness-specific refresh API enforcement
# Then asserts the UI projection layer does not introduce direct authoritative
# publication on its own.

cargo test -p hxrts-aura-app --test compile_fail -- --nocapture
cargo run -q -p hxrts-aura-macros --bin ownership_lints -- \
  harness-readiness-ownership \
  crates/aura-agent/src/reactive/app_signal_views.rs \
  crates/aura-agent/src/reactive/app_signal_projection.rs \
  crates/aura-terminal/src \
  crates/aura-web/src \
  crates/aura-harness/src

ui_violations="$(
  rg -n \
    -e 'publish_authoritative_' \
    -e 'replace_authoritative_semantic_facts_of_kind\(' \
    crates/aura-ui/src \
    -g '*.rs' || true
)"

if [[ -n "$ui_violations" ]]; then
  printf '%s\n' "$ui_violations" >&2
  fail "observed UI modules may not author authoritative semantic truth"
fi

echo "observed layer authorship: clean"
