#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "ownership-category-declarations: $*" >&2
  exit 1
}

ownership_doc="docs/122_ownership_model.md"
project_structure_doc="docs/999_project_structure.md"

[[ -f "$ownership_doc" ]] || fail "missing ownership model doc: $ownership_doc"
[[ -f "$project_structure_doc" ]] || fail "missing project structure doc: $project_structure_doc"

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

mapfile -t crate_dirs < <(find crates -mindepth 1 -maxdepth 1 -type d | sort)
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
    aura-agent|aura-app|aura-terminal|aura-web|aura-harness)
      if ! rg -q '^## Ownership Inventory|^### Ownership Inventory' "$arch_file"; then
        violations+=("$arch_file: high-risk crate missing Ownership Inventory section")
      fi
      ;;
  esac
done

if (( ${#missing_arch[@]} > 0 )); then
  printf '%s\n' "${missing_arch[@]}" >&2
  fail "crates are missing required ARCHITECTURE.md ownership declarations"
fi

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "ownership category declarations are incomplete"
fi

echo "ownership category declarations: clean"
