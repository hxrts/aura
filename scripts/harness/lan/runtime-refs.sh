#!/usr/bin/env bash
# Print the Nix store paths (top-level, deduplicated, sorted) holding the
# dynamic libraries the given binaries load, as the loader reports them
# (`otool -L` on macOS, `ldd` elsewhere). These are the run-time closure
# ship.sh copies to the remote and roots on both hosts (work/8.md Tasks 194
# and 197). Store paths that only appear in a binary's strings (e.g. source
# paths in panic messages) are not run-time dependencies and are skipped.
#
# Usage: scripts/harness/lan/runtime-refs.sh BINARY...
set -euo pipefail
[ "$#" -gt 0 ] || { echo 'usage: runtime-refs.sh BINARY...' >&2; exit 2; }
for bin in "$@"; do
  [ -f "$bin" ] || { echo "runtime-refs: no such file $bin" >&2; exit 1; }
done
if command -v otool >/dev/null 2>&1; then
  libs() { otool -L "$1" | tail -n +2; }
else
  libs() { ldd "$1"; }
fi
for bin in "$@"; do
  libs "$bin"
done | grep -oE '/nix/store/[0-9a-df-np-sv-z]{32}-[A-Za-z0-9+._?=-]+' | sort -u
