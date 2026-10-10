#!/usr/bin/env bash
# Install and set up the Kani verification toolchain if not present.
set -euo pipefail

ROOT="${AURA_KANI_ROOT:-$PWD/.tmp/kani-root}"
BIN="$ROOT/bin"
export KANI_HOME="${AURA_KANI_HOME:-$ROOT/kani-home}"

mkdir -p "$ROOT" "$KANI_HOME"

# Keep installer and reviewed machine-readable discovery on one exact release.
PINNED_VERSION=0.68.0
installed_version() {
    "$BIN/cargo-kani" --version | grep -Eo '[0-9]+\.[0-9]+\.[0-9]+' | head -n1
}
VERSION=''
if [[ -x "$BIN/cargo-kani" ]]; then
    VERSION=$(installed_version) || { echo 'Cannot verify cached Kani version' >&2; exit 2; }
fi
if [[ "$VERSION" != "$PINNED_VERSION" ]]; then
    cargo install --locked --force --version "$PINNED_VERSION" kani-verifier --root "$ROOT"
fi
VERSION=$(installed_version)
[[ "$VERSION" == "$PINNED_VERSION" ]] || { echo 'Installed Kani version differs from canonical pin' >&2; exit 2; }
BUNDLE_DIR="$KANI_HOME/kani-$VERSION"

if [ ! -d "$BUNDLE_DIR" ] || find "$KANI_HOME" -maxdepth 1 -name '*.tar.gz' -print -quit | grep -q .; then
    "$BIN/cargo-kani" setup
fi
