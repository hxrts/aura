#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness tui product path: $*" >&2
  exit 1
}

if rg -q 'AURA_HARNESS_MODE' crates/aura-terminal/src/tui/screens/app/shell.rs; then
  fail "TUI product action dispatch may not branch on AURA_HARNESS_MODE"
fi

if rg -q 'runtime\.(create_contact_invitation|export_invitation|import_invitation|accept_invitation)' \
  crates/aura-terminal/src/tui/screens/app/shell.rs; then
  fail "TUI product action dispatch may not call runtime invitation shortcuts directly"
fi

cargo test -p aura-terminal invitation_dispatch_uses_product_callbacks_without_harness_shortcuts --quiet

echo "harness tui product path: clean"
