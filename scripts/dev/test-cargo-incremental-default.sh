#!/usr/bin/env bash
# Verify the effective compiler default and explicit override without a workspace build.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-incremental-default.XXXXXX")"
trap 'rm -rf "$fixture_root"' EXIT
profile="$(awk '$0 == "[profile.dev]" { active=1; print; next } active && /^\[/ { exit } active { print }' "$repo_root/Cargo.toml")"
[[ "$(printf '%s\n' "$profile" | awk '$1 == "incremental" { print $3 }')" == false ]] || {
  echo 'workspace development profile must default incremental to false' >&2
  exit 1
}
mkdir -p "$fixture_root/src"
cat > "$fixture_root/Cargo.toml" <<'MANIFEST'
[package]
name = "aura_incremental_policy_probe"
version = "0.0.0"
edition = "2021"
[workspace]
MANIFEST
printf '%s\n' "$profile" >> "$fixture_root/Cargo.toml"
printf 'fn main() {}\n' > "$fixture_root/src/main.rs"
export CARGO_TARGET_DIR="$fixture_root/target"
env -u CARGO_INCREMENTAL cargo build --manifest-path "$fixture_root/Cargo.toml" --verbose > "$fixture_root/default.log" 2>&1 || {
  cat "$fixture_root/default.log" >&2; exit 1
}
if rg -q -- '-C incremental=' "$fixture_root/default.log"; then
  echo 'unset CARGO_INCREMENTAL unexpectedly enabled compiler incremental state' >&2
  exit 1
fi
[[ ! -d "$fixture_root/target/debug/incremental" ]] || [[ -z "$(ls -A "$fixture_root/target/debug/incremental")" ]]
CARGO_INCREMENTAL=1 cargo build --manifest-path "$fixture_root/Cargo.toml" --verbose > "$fixture_root/override.log" 2>&1 || {
  cat "$fixture_root/override.log" >&2; exit 1
}
rg -q -- '-C incremental=' "$fixture_root/override.log" || {
  echo 'explicit CARGO_INCREMENTAL=1 override was lost' >&2; exit 1
}
[[ -n "$(ls -A "$fixture_root/target/debug/incremental")" ]]
echo 'Cargo compiler incremental default and explicit override passed'
