#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-onboarding-contract: $*" >&2
  exit 1
}

rg -q 'ScreenId::Onboarding' crates/aura-app/src/ui_contract.rs \
  || fail "onboarding must be declared in the shared snapshot model"
rg -q 'UiSnapshot' crates/aura-web/src/main.rs \
  || fail "web onboarding must publish through UiSnapshot"

hits="$(rg --no-heading -n 'publish_onboarding_snapshot|stale_onboarding_publish|synthetic_onboarding_snapshot' \
  crates/aura-web/src crates/aura-harness/src || true)"

filtered_hits="$(printf '%s\n' "$hits" \
  | grep -v 'crates/aura-web/src/main.rs' \
  | grep -v 'crates/aura-web/src/harness_bridge.rs' \
  | grep -v 'crates/aura-harness/src/backend/local_pty.rs' || true)"
if [ -n "$filtered_hits" ]; then
  echo "$filtered_hits" >&2
  fail "new onboarding-only publication paths are not allowed outside the current quarantine allowlist"
fi

echo "harness onboarding contract: clean"
