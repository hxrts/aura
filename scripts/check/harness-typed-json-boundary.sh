#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-typed-json-boundary: $*" >&2
  exit 1
}

# Shared semantic harness core must decode typed payload structs and enums
# directly. Raw serde_json::Value plumbing is reserved for outer CLI/browser
# boundary adapters, not executor/replay/backend semantic paths.

semantic_core_paths=(
  crates/aura-harness/src/executor.rs
  crates/aura-harness/src/replay.rs
  crates/aura-harness/src/backend/mod.rs
  crates/aura-harness/src/backend/local_pty.rs
  crates/aura-terminal/src/tui/harness_state
  crates/aura-web/src/harness_bridge.rs
)

violations=()
while IFS= read -r match; do
  violations+=("$match")
done < <(
  rg -n \
    -e 'serde_json::Value' \
    -e 'serde_json::from_value\(' \
    "${semantic_core_paths[@]}" \
    -g '*.rs' \
    | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "shared semantic core still relies on raw serde_json::Value plumbing"
fi

echo "harness typed json boundary: clean"
