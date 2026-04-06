#!/usr/bin/env bash
# Check repo hygiene: lonely mod.rs files and empty directories.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_style() {
  section "Rust style — safety and API rules"
  info "Run 'just lint-arch-syntax' for serialized-usize, unit-suffix, and builder-#[must_use] checks."

  local lonely_mods=""
  while IFS= read -r modrs; do
    [[ -z "$modrs" ]] && continue
    local dir
    dir=$(dirname "$modrs")
    local sibling_count
    sibling_count=$(find "$dir" -maxdepth 1 -name "*.rs" ! -name "mod.rs" 2>/dev/null | wc -l | tr -d ' ')
    local subdir_count
    subdir_count=$(find "$dir" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$sibling_count" -eq 0 && "$subdir_count" -eq 0 ]]; then
      lonely_mods+="$modrs"$'\n'
    fi
  done < <(find crates -name "mod.rs" -type f 2>/dev/null)
  emit_hits "Lonely mod.rs (convert to single file)" "$lonely_mods"

  local empty_dirs=""
  while IFS= read -r dir; do
    [[ -z "$dir" ]] && continue
    [[ "$dir" == *".git"* || "$dir" == *"target"* ]] && continue
    git check-ignore -q "$dir" 2>/dev/null && continue
    local file_count
    file_count=$(find "$dir" -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d ' ')
    local subdir_count
    subdir_count=$(find "$dir" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$file_count" -eq 0 && "$subdir_count" -eq 0 ]]; then
      empty_dirs+="$dir"$'\n'
    fi
  done < <(find crates -type d 2>/dev/null)
  emit_hits "Empty directory (delete or add .gitkeep)" "$empty_dirs"

  info "Style checks complete"
}

check_style
