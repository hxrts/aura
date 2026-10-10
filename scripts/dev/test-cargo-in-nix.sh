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
# The shared sccache wrapper passes through by default and AURA_NO_SCCACHE=1
# removes it for one command; a non-sccache wrapper is never touched.
cat > "$fixture_root/cargo" <<'TOOL'
#!/usr/bin/env bash
printf '%s\n' "${RUSTC_WRAPPER:-<none>}" > "$DISPATCH_CAPTURE"
TOOL
RUSTC_WRAPPER=sccache bash "$repo_root/scripts/dev/cargo-in-nix.sh" build
[[ "$(cat "$DISPATCH_CAPTURE")" == sccache ]] || { echo 'sccache wrapper was dropped' >&2; exit 1; }
AURA_NO_SCCACHE=1 RUSTC_WRAPPER=sccache bash "$repo_root/scripts/dev/cargo-in-nix.sh" build
[[ "$(cat "$DISPATCH_CAPTURE")" == '<none>' ]] || { echo 'AURA_NO_SCCACHE=1 kept sccache' >&2; exit 1; }
CARGO_INCREMENTAL=1 RUSTC_WRAPPER=sccache bash "$repo_root/scripts/dev/cargo-in-nix.sh" build
[[ "$(cat "$DISPATCH_CAPTURE")" == '<none>' ]] || { echo 'incremental build kept sccache' >&2; exit 1; }
AURA_NO_SCCACHE=1 RUSTC_WRAPPER=other-wrapper bash "$repo_root/scripts/dev/cargo-in-nix.sh" build
[[ "$(cat "$DISPATCH_CAPTURE")" == other-wrapper ]] || { echo 'unrelated wrapper removed' >&2; exit 1; }
echo 'cargo-in-nix: pinned Clippy dispatch, argument boundaries, exit status and sccache opt-out passed'
# Aggregate toolkit entry point must use the same dispatcher, even when an
# installed toolkit command is present. The non-Nix path must enter Aura first.
mkdir -p "$fixture_root/project/scripts" "$fixture_root/bin" "$fixture_root/toolkit/xtask"
cp "$repo_root/scripts/toolkit-shell.sh" "$fixture_root/project/scripts/toolkit-shell.sh"
cat > "$fixture_root/bin/cargo" <<'TOOL'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$DISPATCH_CAPTURE"
exit 23
TOOL
cat > "$fixture_root/bin/toolkit-clippy" <<'TOOL'
#!/usr/bin/env bash
exit 79
TOOL
cat > "$fixture_root/bin/nix" <<'TOOL'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$DISPATCH_CAPTURE"
exit 31
TOOL
chmod +x "$fixture_root/bin/cargo" "$fixture_root/bin/toolkit-clippy" "$fixture_root/bin/nix"
for shell_state in inside outside; do
  if [[ "$shell_state" == inside ]]; then
    expected_status=23
    printf '%s\n' clippy --manifest-path 'path with spaces/Cargo.toml' -- -D warnings > "$fixture_root/expected"
    nix_state=impure
  else
    expected_status=31
    printf '%s\n' develop "$fixture_root/project" --command cargo clippy --manifest-path 'path with spaces/Cargo.toml' -- -D warnings > "$fixture_root/expected"
    nix_state=''
  fi
  if IN_NIX_SHELL="$nix_state" PATH="$fixture_root/bin:$PATH" bash "$fixture_root/project/scripts/toolkit-shell.sh" toolkit-clippy --manifest-path 'path with spaces/Cargo.toml' -- -D warnings; then
    echo 'toolkit Clippy exit status was lost' >&2; exit 1
  else
    status=$?
    [[ "$status" == "$expected_status" ]] || { echo "unexpected toolkit dispatch status $status" >&2; exit 1; }
  fi
  cmp "$fixture_root/expected" "$DISPATCH_CAPTURE"
done
echo 'toolkit-clippy: pinned shell dispatch, Nix fallback, argument boundaries and exit status passed'
