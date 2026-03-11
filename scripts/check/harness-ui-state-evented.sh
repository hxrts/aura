#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-ui-state-evented: $*" >&2
  exit 1
}

cargo test -p aura-harness wait_contract_refs_cover_all_parity_wait_kinds --quiet
cargo test -p aura-harness semantic_wait_helpers_do_not_use_raw_dom_or_text_fallbacks --quiet
cargo test -p aura-harness raw_text_fallbacks_are_explicitly_diagnostic_only --quiet

cd crates/aura-harness/playwright-driver
node ./playwright_driver.mjs --selftest
cd "$repo_root"

echo "harness ui-state evented policy: clean"
