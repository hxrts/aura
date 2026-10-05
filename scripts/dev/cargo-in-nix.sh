#!/usr/bin/env bash
# Dispatch Clippy from the same pinned Nix toolchain as Cargo and rustc.
# Cargo's external-command lookup prefers installed Cargo-home plugins to PATH.
set -euo pipefail
: "${AURA_NIX_CARGO_BIN:?enter the Aura Nix shell}"
: "${AURA_NIX_CLIPPY_BIN:?enter the Aura Nix shell}"
if [[ "${1:-}" == clippy ]]; then
  exec "$AURA_NIX_CLIPPY_BIN" "$@"
fi
exec "$AURA_NIX_CARGO_BIN" "$@"
