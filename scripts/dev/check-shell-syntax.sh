#!/usr/bin/env bash
# Check every expanded input, including files after the first argument.
set -euo pipefail
(( $# > 0 )) || { echo 'check-shell-syntax: no inputs' >&2; exit 2; }
for script in "$@"; do
  [[ -f "$script" && ! -L "$script" ]] || { echo 'check-shell-syntax: unsafe input' >&2; exit 2; }
  bash -n "$script" || exit
done
