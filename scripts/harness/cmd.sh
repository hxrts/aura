#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: scripts/harness_cmd.sh <subcommand> [args...]" >&2
  exit 2
fi

subcommand="$1"
shift

if [ "${1:-}" = "--" ]; then
  shift
fi

cargo run -p aura-harness --bin aura-harness -- "$subcommand" "$@"
