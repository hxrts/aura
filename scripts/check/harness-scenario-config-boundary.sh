#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-scenario-config-boundary: $*" >&2
  exit 1
}

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

if rg -n '^\s*id\s*=\s*"(web|tui|browser|local|playwright|pty)"' configs/harness -g '*.toml' >"$tmp"; then
  cat "$tmp" >&2
  fail "config instance ids must remain actor-based and frontend-neutral"
fi

if ! rg -n '^\s*mode\s*=\s*"(local|browser|ssh)"' configs/harness -g '*.toml' >"$tmp"; then
  fail "expected config frontend/runtime bindings declared via instance.mode"
fi

echo "harness scenario/config boundary: clean"
