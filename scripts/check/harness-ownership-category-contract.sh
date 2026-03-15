#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-ownership-category-contract: $*" >&2
  exit 1
}

testing_guide="docs/804_testing_guide.md"
app_arch="crates/aura-app/ARCHITECTURE.md"
terminal_arch="crates/aura-terminal/ARCHITECTURE.md"
web_arch="crates/aura-web/ARCHITECTURE.md"
harness_arch="crates/aura-harness/ARCHITECTURE.md"
agent_arch="crates/aura-agent/ARCHITECTURE.md"

for file in "$testing_guide" "$app_arch" "$terminal_arch" "$web_arch" "$harness_arch" "$agent_arch"; do
  [[ -f "$file" ]] || fail "missing required ownership-model file: $file"
done

rg -q '### Shared Semantic Ownership Inventory' "$testing_guide" \
  || fail "testing guide must define the shared semantic ownership inventory"

required_inventory_rows=(
  'Semantic command / handle contract'
  'Semantic operation lifecycle'
  'Channel / invitation / delivery readiness'
  'Runtime-facing async service state'
  'TUI command ingress'
  'TUI shell / callbacks / subscriptions'
  'Browser harness bridge'
  'Harness executor / wait model'
  'Ownership transfer / stale-owner invalidation'
)

for row in "${required_inventory_rows[@]}"; do
  rg -Fq "$row" "$testing_guide" \
    || fail "testing guide ownership inventory missing row: $row"
done

for file in "$app_arch" "$terminal_arch" "$web_arch" "$harness_arch"; do
  rg -q '^## Ownership Model' "$file" \
    || fail "missing Ownership Model section in $file"
done

rg -q 'Structured Concurrency Model' "$agent_arch" \
  || fail "aura-agent architecture doc must define the structured concurrency model"
rg -q 'Session Ownership' "$agent_arch" \
  || fail "aura-agent architecture doc must define session ownership"

echo "harness ownership category contract: clean"
