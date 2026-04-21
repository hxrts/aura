#!/usr/bin/env bash
# Build TUI binaries and run the harness matrix for a given suite.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mkdir -p artifacts/harness/tui
log_file="$repo_root/artifacts/harness/tui/ci-matrix-tui.log"
: >"$log_file"
exec >>"$log_file" 2>&1

cargo build -p aura-terminal --bin aura -q
cargo build -p aura-harness --bin aura-harness -q

export AURA_HARNESS_BIN="$repo_root/target/debug/aura-harness"
export AURA_HARNESS_MATRIX_LOG_FILE="$repo_root/artifacts/harness/tui/matrix-tui.log"

bash scripts/harness/run-matrix.sh --lane tui "$@"
