#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

if (( $# == 0 )); then
  echo "usage: harness-governance.sh <subcommand>" >&2
  echo "subcommands: scenario-legality, scenario-shape-contract, core-scenario-mechanics, shared-scenario-contract" >&2
  exit 1
fi

cargo run -p aura-harness --bin aura-harness --quiet -- governance "$1"
