#!/usr/bin/env bash
# Publishable crates in dependency order (leaves first, L1 → L8).
# Non-publishable crates (test infra, examples, internal tools) are excluded.

RELEASE_PACKAGES=(
  # L1 Foundation
  "aura-core"

  # L2 Specification
  "aura-macros"
  "aura-journal"
  "aura-authorization"
  "aura-signature"
  "aura-store"
  "aura-transport"
  "aura-maintenance"
  "aura-mpst"

  # L3 Implementation
  "aura-effects"
  "aura-composition"

  # L4 Orchestration
  "aura-guards"
  "aura-consensus"
  "aura-amp"
  "aura-anti-entropy"
  "aura-protocol"

  # L5 Features
  "aura-authentication"
  "aura-chat"
  "aura-invitation"
  "aura-recovery"
  "aura-relational"
  "aura-rendezvous"
  "aura-social"
  "aura-sync"

  # L6 Runtime
  "aura-app"
  "aura-agent"
  "aura-simulator"

  # L7 Interface
  "aura-ui"
  "aura-terminal"
  # aura-web is WASM-only and not published to crates.io
)

# Not published:
#   aura-web       — WASM browser shell, not a library crate
#   aura-testkit   — test infrastructure
#   aura-quint     — verification tooling
#   aura-harness   — test harness
#   examples/*     — example crates

manifest_path() {
  local crate="$1"
  local path="crates/${crate}/Cargo.toml"
  if [[ -f "${path}" ]]; then
    echo "${path}"
  else
    return 1
  fi
}
