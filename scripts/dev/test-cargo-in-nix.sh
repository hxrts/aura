#!/usr/bin/env bash
# Isolated regression: a stale Cargo-home plugin must not select the compiler.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-cargo-dispatch.XXXXXX")"
trap 'rm -rf "$fixture_root"' EXIT
export DISPATCH_CAPTURE="$fixture_root/arguments"
cat > "$fixture_root/cargo" <<'TOOL'
#!/usr/bin/env bash
if [[ "${1:-}" == clippy ]]; then exit 79; fi
printf '%s\n' "$@" > "$DISPATCH_CAPTURE"
exit 0
TOOL
cat > "$fixture_root/cargo-clippy" <<'TOOL'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$DISPATCH_CAPTURE"
exit 23
TOOL
chmod +x "$fixture_root/cargo" "$fixture_root/cargo-clippy"
export AURA_NIX_CARGO_BIN="$fixture_root/cargo"
export AURA_NIX_CLIPPY_BIN="$fixture_root/cargo-clippy"
if bash "$repo_root/scripts/dev/cargo-in-nix.sh" clippy --workspace -- -D warnings; then
  echo 'Clippy exit status was lost' >&2; exit 1
else
  status=$?
  [[ "$status" == 23 ]] || { echo "unexpected dispatch status $status" >&2; exit 1; }
fi
printf '%s\n' clippy --workspace -- -D warnings > "$fixture_root/expected"
cmp "$fixture_root/expected" "$DISPATCH_CAPTURE"
bash "$repo_root/scripts/dev/cargo-in-nix.sh" test --manifest-path 'path with spaces/Cargo.toml'
printf '%s\n' test --manifest-path 'path with spaces/Cargo.toml' > "$fixture_root/expected"
cmp "$fixture_root/expected" "$DISPATCH_CAPTURE"
echo 'cargo-in-nix: pinned Clippy dispatch, argument boundaries and exit status passed'
