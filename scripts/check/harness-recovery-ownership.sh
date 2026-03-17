#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-recovery-ownership: $*" >&2
  exit 1
}

observation_files=(
  crates/aura-harness/src/tool_api.rs
  crates/aura-terminal/src/tui/harness_state/snapshot.rs
  crates/aura-ui/src/model.rs
  crates/aura-web/src/harness_bridge.rs
)

hits="$(rg --no-heading -n \
  'std::thread::sleep|thread::sleep|tokio::time::sleep|run_registered_recovery|retry|fallback' \
  "${observation_files[@]}" || true)"
if [ -n "$hits" ]; then
  echo "$hits" >&2
  fail "parity-critical observation code may not introduce sleeps, retries, or recovery helpers outside approved owner modules"
fi

echo "harness recovery ownership: clean"
