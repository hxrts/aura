#!/usr/bin/env bash
# Verify the actual deployable dependency feature graphs cannot enable the
# simulator-only profile/nonce checkpoint owner or test-keyring adapter.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
graph="$(mktemp)"
trap 'rm -f "$graph"' EXIT

check_graph() {
  local name="$1"
  shift
  cargo tree --locked -e normal,build,features "$@" > "$graph"
  if rg -q 'feature "(simulator-checkpoint|test-keyring|os-keychain-diagnostic)"|aura-simulator v' "$graph"; then
    printf 'simulator checkpoint reachable from deployable %s feature graph\n' "$name" >&2
    rg -n 'feature "(simulator-checkpoint|test-keyring|os-keychain-diagnostic)"|aura-simulator v' "$graph" >&2
    return 1
  fi
  printf 'simulator checkpoint absent from %s feature graph\n' "$name"
}

check_graph terminal-and-rpc -p aura-terminal --no-default-features --features terminal
check_graph app-host -p hxrts-aura-app --features host
check_graph web-release --target wasm32-unknown-unknown -p aura-web --features web,harness
