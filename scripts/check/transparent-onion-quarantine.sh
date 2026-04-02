#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "transparent-onion-quarantine: $*" >&2
  exit 1
}

lane_files=(
  ".github/workflows/ci.yml"
  ".github/workflows/harness.yml"
  "justfile"
  "scripts/check/shared-flow-policy.sh"
  "scripts/check/user-flow-policy-guardrails.sh"
  "scripts/ci/harness-browser.sh"
  "scripts/ci/harness-matrix-web.sh"
  "scripts/ci/harness-matrix-tui.sh"
  "scripts/ci/harness-shared-semantic-web.sh"
  "scripts/ci/harness-shared-semantic-tui.sh"
  "scripts/ci/harness-frontend-conformance-web.sh"
  "scripts/ci/harness-frontend-conformance-tui.sh"
)

violations="$(
  rg -n 'transparent_onion|--features[ =][^\\n]*transparent_onion' "${lane_files[@]}" || true
)"
if [[ -n "$violations" ]]; then
  echo "$violations" >&2
  fail "harness and shared-flow lanes must not enable or depend on transparent_onion"
fi

allowed_source_files=(
  "crates/aura-core/src/service.rs"
  "crates/aura-core/src/lib.rs"
  "crates/aura-agent/src/lib.rs"
  "crates/aura-effects/src/lib.rs"
  "crates/aura-protocol/src/lib.rs"
  "crates/aura-social/src/lib.rs"
  "crates/aura-sync/src/lib.rs"
)

source_violations="$(
  rg -n 'TransparentAnonymousSetup|TransparentMoveEnvelope|TransparentMoveTrafficClass|transparent_headers|PathProtectionMode::TransparentDebug|feature *= *"transparent_onion"' crates \
    $(printf " --glob '!%s'" "${allowed_source_files[@]}") || true
)"
if [[ -n "$source_violations" ]]; then
  echo "$source_violations" >&2
  fail "transparent debug surfaces must remain quarantined to the explicit allowlist"
fi

echo "transparent-onion-quarantine: ok"
