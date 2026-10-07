#!/usr/bin/env bash
# Run `cargo check ARGS...` and fail if any workspace crate emits a warning.
# This denies warnings without RUSTFLAGS, which would change every
# dependency's fingerprint and force a separate dependency build per lane.
set -euo pipefail
log="$(mktemp "${TMPDIR:-/tmp}/aura-check-warnings.XXXXXX")"
trap 'rm -f "$log"' EXIT
status=0
cargo check "$@" 2>&1 | tee "$log" || status=$?
if [[ "$status" -ne 0 ]]; then exit "$status"; fi
# Cargo replays cached warnings, and dependencies are capped by --cap-lints,
# so any warning line here comes from a workspace crate.
if grep -Eq '^warning(\[|:)' "$log"; then
  echo 'cargo-check-no-warnings: warnings are denied' >&2
  exit 1
fi
