#!/usr/bin/env bash
# Verify the excluded host-only toolkit/test-support package resolves to its
# own workspace when this checkout is nested inside another checkout
# (.claude/worktrees/agent-*), so fmt/clippy toolkit recipes work there.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-nested-worktree.XXXXXX")"
fixture_root="$(cd "$fixture_root" && pwd -P)"
trap 'rm -rf "$fixture_root"' EXIT
outer="$fixture_root/outer"
nested="$outer/.claude/worktrees/agent-fixture"
package="$nested/toolkit/test-support"
mkdir -p "$package"
cp "$repo_root/Cargo.toml" "$outer/Cargo.toml"
cp "$repo_root/Cargo.toml" "$nested/Cargo.toml"
cp "$repo_root/toolkit/test-support/Cargo.toml" "$package/Cargo.toml"
: > "$package/process_lock.rs"
locate() { (cd "$package" && cargo locate-project --workspace --message-format plain 2>&1); }

if ! actual="$(locate)" || [[ "$actual" != "$package/Cargo.toml" ]]; then
  echo "nested worktree: toolkit/test-support resolved to '$actual', expected its own manifest" >&2
  exit 1
fi
# Fixture fidelity: without its own [workspace] table the package escapes to
# the enclosing checkout, which is the failure this guards against.
grep -v '^\[workspace\]$' "$repo_root/toolkit/test-support/Cargo.toml" > "$package/Cargo.toml"
if actual="$(locate)" && [[ "$actual" == "$package/Cargo.toml" ]]; then
  echo "nested worktree fixture no longer reproduces the enclosing-workspace escape" >&2
  exit 1
fi
echo "nested worktree manifests: ok"
