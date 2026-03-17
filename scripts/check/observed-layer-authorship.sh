#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "observed-layer-authorship: $*" >&2
  exit 1
}

# Composition script that delegates to:
#   1. authoritative-fact-authorship.sh — lifecycle/readiness publication boundaries
#      (also covers semantic lifecycle ownership since the merge)
#   2. harness-readiness-ownership.sh — readiness-specific refresh API enforcement
# Then asserts the UI projection layer does not introduce direct authoritative
# publication on its own.

bash scripts/check/authoritative-fact-authorship.sh
bash scripts/check/harness-readiness-ownership.sh

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
