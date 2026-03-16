#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "timeout-time-domain-usage: $*" >&2
  exit 1
}

# Layer rule: semantic/domain/orchestration/feature crates should not encode
# wall-clock timeout semantics directly in production source. Those layers
# should use Aura's typed time abstractions and leave local timeout budgeting to
# the sanctioned runtime/interface helpers.

violations="$(
  {
    rg -n \
      -e 'tokio::time::timeout\(' \
      -e 'tokio::time::sleep\(' \
      -e 'SystemTime::now\(' \
      -e 'Instant::now\(' \
      crates/aura-{journal,authorization,signature,store,transport,mpst,macros,protocol,guards,consensus,amp,anti-entropy,authentication,chat,invitation,recovery,relational,rendezvous,social,sync}/src \
      -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*' || true
)"

if [[ -n "$violations" ]]; then
  printf '%s\n' "$violations" >&2
  fail "semantic layers are using direct wall-clock timeout primitives instead of typed time domains"
fi

echo "timeout time-domain usage: clean"
