#!/usr/bin/env bash
# Validate actor-vs-move ownership inventory alignment across docs and crates.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Governance-only check. This keeps the shared semantic ownership docs and the
# composed harness ownership inventory aligned, but compile-time ownership
# boundaries live in the typed APIs, macros, and compile-fail suites.

fail() {
  echo "harness-actor-vs-move-ownership: $*" >&2
  exit 1
}

testing_guide="docs/804_testing_guide.md"
app_arch="crates/aura-app/ARCHITECTURE.md"
terminal_arch="crates/aura-terminal/ARCHITECTURE.md"
web_arch="crates/aura-web/ARCHITECTURE.md"
harness_arch="crates/aura-harness/ARCHITECTURE.md"
agent_arch="crates/aura-agent/ARCHITECTURE.md"

for file in "$testing_guide" "$app_arch" "$terminal_arch" "$web_arch" "$harness_arch" "$agent_arch"; do
  [[ -f "$file" ]] || fail "missing required ownership-model doc: $file"
done

rg -q 'Shared Semantic Ownership Model' "$testing_guide" \
  || fail "testing guide must define the shared semantic ownership model"
rg -q '`Pure`' "$testing_guide" || fail "testing guide must mention Pure ownership"
rg -q '`MoveOwned`' "$testing_guide" || fail "testing guide must mention MoveOwned ownership"
rg -q '`ActorOwned`' "$testing_guide" || fail "testing guide must mention ActorOwned ownership"
rg -q '`Observed`' "$testing_guide" || fail "testing guide must mention Observed ownership"

rg -q '## Ownership Model' "$app_arch" \
  || fail "aura-app architecture doc must define an Ownership Model section"
rg -q 'primarily a `Pure` plus `MoveOwned`' "$app_arch" \
  || fail "aura-app must declare its Pure + MoveOwned role"
rg -q 'not `ActorOwned`' "$app_arch" \
  || fail "aura-app must explicitly reject ActorOwned runtime ownership"

rg -q '## Ownership Model' "$terminal_arch" \
  || fail "aura-terminal architecture doc must define an Ownership Model section"
rg -q '`Observed`' "$terminal_arch" \
  || fail "aura-terminal must declare its Observed role"
rg -q 'must not own' "$terminal_arch" \
  || fail "aura-terminal must explicitly reject terminal semantic truth ownership"

rg -q '## Ownership Model' "$web_arch" \
  || fail "aura-web architecture doc must define an Ownership Model section"
rg -q '`Observed`' "$web_arch" \
  || fail "aura-web must declare its Observed role"
rg -q 'must not own' "$web_arch" \
  || fail "aura-web must explicitly reject terminal semantic lifecycle ownership"

rg -q '## Ownership Model' "$harness_arch" \
  || fail "aura-harness architecture doc must define an Ownership Model section"
rg -q '`Observed`' "$harness_arch" \
  || fail "aura-harness must declare its Observed role"
rg -q 'must not author semantic lifecycle truth' "$harness_arch" \
  || fail "aura-harness must explicitly reject semantic lifecycle authorship"

rg -q 'actor services solve long-lived runtime supervision and lifecycle' "$agent_arch" \
  || fail "aura-agent must document actor ownership for runtime structure"
rg -q 'move semantics solve session and endpoint ownership transfer' "$agent_arch" \
  || fail "aura-agent must document move semantics for ownership transfer"

# Composition: delegates to sub-checks after the doc validation above.
#   1. ownership_lints harness-readiness-ownership — readiness-specific refresh API enforcement
#   2. ownership_lints harness-move-ownership-boundary — frontend handle/receipt fabrication boundaries
# Note: semantic lifecycle authorship is now covered by aura-app compile-fail ownership boundaries.
cargo run -q -p hxrts-aura-macros --bin ownership_lints -- \
  harness-readiness-ownership \
  crates/aura-agent/src/reactive/app_signal_views.rs \
  crates/aura-agent/src/reactive/app_signal_projection.rs \
  crates/aura-terminal/src \
  crates/aura-web/src \
  crates/aura-harness/src
cargo run -q -p hxrts-aura-macros --bin ownership_lints -- \
  harness-move-ownership-boundary \
  crates/aura-app \
  crates/aura-terminal \
  crates/aura-web \
  crates/aura-harness

echo "harness actor-vs-move ownership: clean"
