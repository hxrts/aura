#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

local_toolkit="$(cd "${repo_root}/.." && pwd)/toolkit"
if [ -d "${local_toolkit}/xtask" ]; then
  export TOOLKIT_ROOT="${local_toolkit}"
fi

if [ -n "${IN_NIX_SHELL:-}" ] && [ -n "${TOOLKIT_ROOT:-}" ] && command -v toolkit-xtask >/dev/null 2>&1; then
  exec "$@"
fi

if [ -n "${TOOLKIT_ROOT:-}" ] && [ -f "${TOOLKIT_ROOT}/flake.nix" ]; then
  exec nix develop "${TOOLKIT_ROOT}" --command "$@"
fi

exec nix develop --command "$@"
