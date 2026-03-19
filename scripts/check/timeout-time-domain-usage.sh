#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo run -q -p aura-macros --bin ownership_lints -- \
  time-domain-usage \
  crates/aura-journal/src \
  crates/aura-authorization/src \
  crates/aura-signature/src \
  crates/aura-store/src \
  crates/aura-transport/src \
  crates/aura-mpst/src \
  crates/aura-macros/src \
  crates/aura-protocol/src \
  crates/aura-guards/src \
  crates/aura-consensus/src \
  crates/aura-amp/src \
  crates/aura-anti-entropy/src \
  crates/aura-authentication/src \
  crates/aura-chat/src \
  crates/aura-invitation/src \
  crates/aura-recovery/src \
  crates/aura-relational/src \
  crates/aura-rendezvous/src \
  crates/aura-social/src \
  crates/aura-sync/src
