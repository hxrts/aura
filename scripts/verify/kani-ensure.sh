#!/usr/bin/env bash
# Install and set up the Kani verification toolchain if not present.
set -euo pipefail

ROOT="${AURA_KANI_ROOT:-$PWD/.tmp/kani-root}"
BIN="$ROOT/bin"
export KANI_HOME="${AURA_KANI_HOME:-$ROOT/kani-home}"

mkdir -p "$ROOT" "$KANI_HOME"

if [ ! -x "$BIN/cargo-kani" ]; then
    cargo install --locked kani-verifier --root "$ROOT"
fi

VERSION="$("$BIN/cargo-kani" --version | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -n1)"
BUNDLE_DIR="$KANI_HOME/kani-$VERSION"

if [ ! -d "$BUNDLE_DIR" ] || find "$KANI_HOME" -maxdepth 1 -name '*.tar.gz' -print -quit | grep -q .; then
    "$BIN/cargo-kani" setup
fi
