#!/usr/bin/env bash
# Validate ownership category declarations in docs and crate inventories.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Governance-only check. This validates that docs and crate inventories still
# declare the final ownership categories, but it is not a primary code-
# correctness enforcement mechanism.

fail() {
  echo "ownership-category-declarations: $*" >&2
  exit 1
}

ownership_doc="docs/122_ownership_model.md"
project_structure_doc="docs/999_project_structure.md"
testing_guide="docs/804_testing_guide.md"

[[ -f "$ownership_doc" ]] || fail "missing ownership model doc: $ownership_doc"
[[ -f "$project_structure_doc" ]] || fail "missing project structure doc: $project_structure_doc"
[[ -f "$testing_guide" ]] || fail "missing testing guide: $testing_guide"

required_docs=(
  "$ownership_doc"
  "$project_structure_doc"
  "docs/001_system_architecture.md"
  "docs/103_effect_system.md"
  "docs/104_runtime.md"
)

for file in "${required_docs[@]}"; do
  [[ -f "$file" ]] || fail "missing required ownership guidance doc: $file"
done

for category in '`Pure`' '`MoveOwned`' '`ActorOwned`' '`Observed`'; do
  rg -Fq "$category" "$ownership_doc" \
    || fail "ownership model doc missing category declaration: $category"
done

# Testing guide shared semantic ownership inventory
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

crate_dirs=()
while IFS= read -r crate_dir; do
  crate_dirs+=("$crate_dir")
done < <(find crates -mindepth 1 -maxdepth 1 -type d | sort)
(( ${#crate_dirs[@]} > 0 )) || fail "no crate directories found under crates/"

missing_arch=()
violations=()

for crate_dir in "${crate_dirs[@]}"; do
  crate_name="$(basename "$crate_dir")"
  cargo_toml="$crate_dir/Cargo.toml"
  src_dir="$crate_dir/src"
  arch_file="$crate_dir/ARCHITECTURE.md"

  [[ -f "$cargo_toml" ]] || continue
  [[ -d "$src_dir" ]] || continue

  if [[ ! -f "$arch_file" ]]; then
    missing_arch+=("$arch_file")
    continue
  fi

  if ! rg -q '^## Ownership Model|^## Ownership Inventory|^### Ownership Inventory' "$arch_file"; then
    violations+=("$arch_file: missing ownership section")
  fi

  if ! rg -q '`Pure`|`MoveOwned`|`ActorOwned`|`Observed`' "$arch_file"; then
    violations+=("$arch_file: missing explicit ownership category declarations")
  fi

  case "$crate_name" in
    aura-agent|aura-app|aura-terminal|aura-web|aura-harness|aura-ui|aura-simulator|aura-testkit)
      if ! rg -q '^## Ownership Inventory|^### Ownership Inventory|^### Inventory' "$arch_file"; then
        violations+=("$arch_file: high-risk crate missing Ownership Inventory section")
      fi
      ;;
  esac

  case "$crate_name" in
    aura-authentication|aura-chat|aura-invitation|aura-recovery|aura-relational|aura-rendezvous|aura-social|aura-sync)
      lib_file="$src_dir/lib.rs"
      if [[ ! -f "$lib_file" ]]; then
        violations+=("$crate_dir: Layer 5 crate missing src/lib.rs for OPERATION_CATEGORIES enforcement")
      elif ! rg -q 'pub const OPERATION_CATEGORIES' "$lib_file"; then
        violations+=("$lib_file: Layer 5 crate missing OPERATION_CATEGORIES declaration")
      fi
      if ! rg -q 'OPERATION_CATEGORIES' "$arch_file"; then
        violations+=("$arch_file: Layer 5 crate must document OPERATION_CATEGORIES linkage")
      fi
      ;;
  esac
done

# Agent-specific structural concurrency checks
agent_arch="crates/aura-agent/ARCHITECTURE.md"
if [[ -f "$agent_arch" ]]; then
  rg -q 'Structured Concurrency Model' "$agent_arch" \
    || violations+=("$agent_arch: must define the structured concurrency model")
  rg -q 'Session Ownership' "$agent_arch" \
    || violations+=("$agent_arch: must define session ownership")
fi

if (( ${#missing_arch[@]} > 0 )); then
  printf '%s\n' "${missing_arch[@]}" >&2
  fail "crates are missing required ARCHITECTURE.md ownership declarations"
fi

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "ownership category declarations are incomplete"
fi

echo "ownership category declarations: clean"
