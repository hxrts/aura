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
if [ -n "$hits" ]; then
  echo "$hits" >&2
  fail "onboarding must not introduce bespoke publication or recovery hooks"
fi

if rg -q 'reset_harness_bootstrap_storage_once' crates/aura-web/src/main.rs; then
  fail "web onboarding may not carry harness-only bootstrap reset shortcuts"
fi

echo "harness onboarding contract: clean"
