#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-export-override-policy: $*" >&2
  exit 1
}

hits="$(rg --no-heading -n 'publish_.*override|_override\(' \
  crates/aura-terminal/src/tui \
  crates/aura-ui/src \
  crates/aura-web/src || true)"

filtered_hits="$(printf '%s\n' "$hits" | grep -v 'crates/aura-terminal/src/tui/harness_state.rs' || true)"
if [ -n "$filtered_hits" ]; then
  echo "$filtered_hits" >&2
  fail "new parity-critical export helpers may not depend on override caches outside the quarantined TUI harness export module"
fi

echo "harness export override policy: clean"
