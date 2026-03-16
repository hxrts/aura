#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "observed-layer-authorship: $*" >&2
  exit 1
}

# Keep this thin: compose the stricter ownership checks that already police
# lifecycle/readiness/publication boundaries, then assert the UI projection
# layer is not introducing direct authoritative publication on its own.

bash scripts/check/authoritative-fact-authorship.sh
bash scripts/check/harness-semantic-lifecycle-ownership.sh
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
