#!/usr/bin/env bash
# Run TUI shared semantic test suite via the harness matrix runner.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

bash scripts/ci/tui-matrix.sh --suite shared "$@"
