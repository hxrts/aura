#!/usr/bin/env bash
# Check layer purity and dependency direction.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_layers() {
  section "Layer purity — aura-core interface-only; impls in aura-effects or domain crates"

  if grep -RE "\bimpl\b.*Effects" crates/aura-core/src 2>/dev/null \
    | grep -v "trait" \
    | grep -v "impl<" \
    | grep -v "ScriptedTimeEffects" \
    | grep -v ":///" >/dev/null; then
    violation "aura-core contains effect implementations (should be interface-only)"
  else
    info "aura-core: interface-only (no effect impls)"
  fi

  for crate in aura-authentication aura-app aura-chat aura-invitation aura-recovery aura-relational aura-rendezvous aura-sync; do
    [[ -d "crates/$crate" ]] || continue
    if grep -A20 "^\[dependencies\]" "crates/$crate/Cargo.toml" | grep -E "aura-agent|aura-simulator|aura-terminal" >/dev/null; then
      violation "$crate depends on runtime/UI layers"
    else
      info "$crate: no runtime/UI deps"
    fi
  done

  section "Layer 4 lint policy — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for crate-level allow-attribute enforcement."
}

check_deps() {
  section "Dependency direction — no upward deps (Lx→Ly where y>x)"

  if check_cargo; then
    if ! command -v jq >/dev/null 2>&1; then
      violation "jq unavailable; dependency direction not checked"
      return
    fi
    local deps clean=true
    deps=$(cargo metadata --format-version 1 --no-deps 2>/dev/null | jq -r '.packages[] | select(.name | startswith("aura-")) | [.name, (.dependencies[] | select(.name | startswith("aura-")) | .name)] | @tsv') || deps=""
    while IFS=$'\t' read -r src dst; do
      [[ -z "$src" ]] && continue
      local src_l=$(layer_of "$src") dst_l=$(layer_of "$dst")
      if [[ "$src_l" -gt 0 && "$dst_l" -gt 0 && "$dst_l" -gt "$src_l" ]]; then
        violation "$src (L$src_l) depends upward on $dst (L$dst_l)"
        clean=false
      fi
    done <<< "$deps"
    $clean && info "Dependency direction: clean"
  else
    violation "cargo unavailable; dependency direction not checked"
  fi

  section "Layer 4 firewall — no deps on L6+"
  local l4_crates=(aura-protocol aura-guards aura-consensus aura-amp aura-anti-entropy)
  local blocked="aura-agent|aura-simulator|aura-app|aura-terminal|aura-testkit"
  for crate in "${l4_crates[@]}"; do
    [[ -f "crates/$crate/Cargo.toml" ]] || continue
    if rg "^[^#]*($blocked)" "crates/$crate/Cargo.toml" >/dev/null 2>&1; then
      violation "$crate depends on L6+ — forbidden"
    else
      info "$crate: firewall clean"
    fi
  done
}

check_layers
check_deps
