#!/usr/bin/env bash
# Fail when the committed Cargo.nix no longer matches the committed Cargo.lock.
#
# Cargo.nix is generated from Cargo.lock by crate2nix and both are tracked, so
# a dependency change that regenerates one without the other is a drift between
# the Cargo build graph and the hermetic Nix build graph. Regenerate with
# `just generate-cargo-nix` and commit the result.
#
# crate2nix records its command line in the header and emits source paths
# relative to the output file, so the check regenerates ./Cargo.nix in place
# exactly as `just generate-cargo-nix` does, compares, and always restores the
# committed file.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

for tracked in Cargo.lock Cargo.nix; do
  if ! git ls-files --error-unmatch "$tracked" >/dev/null 2>&1; then
    echo "cargo-nix-sync: $tracked is not tracked by git" >&2
    exit 1
  fi
done

command -v crate2nix >/dev/null 2>&1 || {
  echo "cargo-nix-sync: crate2nix not on PATH (run inside nix develop)" >&2
  exit 1
}

if ! git diff --quiet -- Cargo.nix; then
  echo "cargo-nix-sync: Cargo.nix has uncommitted modifications; commit or revert them first" >&2
  exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/cargo-nix-sync.XXXXXX")"
cp Cargo.nix "$work/committed"
restore() {
  cp "$work/committed" Cargo.nix
  rm -rf "$work"
}
trap restore EXIT

crate2nix generate >/dev/null

if ! diff -u "$work/committed" Cargo.nix > "$work/diff.txt"; then
  echo "cargo-nix-sync: Cargo.nix is out of date relative to Cargo.lock" >&2
  echo "  regenerate with: just generate-cargo-nix" >&2
  head -60 "$work/diff.txt" >&2
  exit 1
fi
echo "cargo-nix-sync: Cargo.nix matches Cargo.lock"
