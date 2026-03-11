#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness onboarding publication: $*" >&2
  exit 1
}

if rg -q 'publish_onboarding_snapshot' crates/aura-web/src/main.rs; then
  fail "web onboarding may not publish through a bespoke snapshot path"
fi

if rg -q 'stale_onboarding_publish' crates/aura-web/src/harness_bridge.rs; then
  fail "browser harness bridge may not carry stale-onboarding publication recovery"
fi

if rg -q 'staleOnboardingCache|stale_onboarding_' crates/aura-harness/playwright-driver/playwright_driver.mjs; then
  fail "playwright driver may not carry stale-onboarding recovery heuristics"
fi

if rg -q 'synthetic_onboarding_snapshot' crates/aura-harness/src/backend/local_pty.rs; then
  fail "local PTY backend may not fabricate onboarding snapshots"
fi

cargo test -p aura-app onboarding_is_declared_in_the_shared_snapshot_model --quiet
cargo test -p aura-app onboarding_uses_canonical_snapshot_publication_path --quiet
cargo test -p aura-app onboarding_harness_paths_have_no_bespoke_recovery_logic --quiet

echo "harness onboarding publication: clean"
