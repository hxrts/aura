#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo run -p aura-harness --bin aura-harness --quiet -- governance legacy-shared-quarantine
