#!/usr/bin/env bash
# Validate ARCHITECTURE.md invariant sections across crates.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_invariants() {
  section "Invariant docs — crate ARCHITECTURE.md must define invariant sections"

  local arch_files
  arch_files=$(find crates -maxdepth 2 -name ARCHITECTURE.md 2>/dev/null | sort)
  [[ -z "$arch_files" ]] && { violation "No crate ARCHITECTURE.md files found"; return; }

  local with_invariants=0 with_detailed=0
  for arch in $arch_files; do
    if rg -q "^## Invariants" "$arch"; then
      ((with_invariants+=1))
      info "Invariants section: $arch"
    fi

    if rg -q "^### Detailed Specifications$|^## Detailed Invariant Specifications$|^### Invariant" "$arch"; then
      ((with_detailed+=1))
      local missing=()
      rg -qi "Enforcement locus:" "$arch" || missing+=("Enforcement locus")
      rg -qi "Failure mode:" "$arch" || missing+=("Failure mode")
      rg -qi "Verification hooks:" "$arch" || missing+=("Verification hooks")
      if [[ ${#missing[@]} -gt 0 ]]; then
        violation "Missing detailed invariant fields [$(IFS=,; echo "${missing[*]}")]: $arch"
      else
        info "Detailed invariant fields: $arch"
      fi
    fi
  done

  if [[ "$with_invariants" -eq 0 ]]; then
    violation "No crate ARCHITECTURE.md includes an Invariants section"
  fi
  if [[ "$with_detailed" -eq 0 ]]; then
    info "No crate has detailed invariant specs yet"
  fi
}

check_invariants
