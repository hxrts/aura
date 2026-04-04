#!/usr/bin/env bash
# Ensure observation paths are free of non-deterministic time, random, or UUID calls.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-observation-determinism: $*" >&2
  exit 1
}

rust_observation_files=(
  crates/aura-terminal/src/tui/harness_state/snapshot.rs
  crates/aura-ui/src/model/mod.rs
  crates/aura-web/src/harness_bridge.rs
)

rust_hits="$(rg --no-heading -n \
  'SystemTime::now|Instant::now|std::time::SystemTime|std::time::Instant|chrono::Utc::now|chrono::Local::now|thread_rng\(\)|rand::thread_rng|rand::random|getrandom::|OsRng|Uuid::new_v4' \
  "${rust_observation_files[@]}" || true)"
if [ -n "$rust_hits" ]; then
  echo "$rust_hits" >&2
  fail "parity-critical observation paths may not read wall clock time, unseeded randomness, or nondeterministic ids"
fi

js_hits="$(rg --no-heading -n 'Math\.random|randomUUID\(' \
  crates/aura-harness/playwright-driver/playwright_driver.mjs || true)"
if [ -n "$js_hits" ]; then
  echo "$js_hits" >&2
  fail "browser observation path may not use JS randomness in parity-critical observation code"
fi

echo "harness observation determinism: clean"
