#!/usr/bin/env bash
# Build TUI binaries and run the harness matrix for a given suite.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo build -p aura-terminal --bin aura -q
cargo build -p aura-harness --bin aura-harness -q

export AURA_HARNESS_BIN="$repo_root/target/debug/aura-harness"

bash scripts/harness/run-matrix.sh --lane tui "$@"
