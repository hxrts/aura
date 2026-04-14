#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

local_toolkit="$(cd "${repo_root}/.." && pwd)/toolkit"
if [ -d "${local_toolkit}/xtask" ]; then
  export TOOLKIT_ROOT="${local_toolkit}"
fi

requested_command="${1:-}"

can_exec_directly=0
if [ -n "${IN_NIX_SHELL:-}" ] && [ -n "${TOOLKIT_ROOT:-}" ] && [ -n "${requested_command}" ] && command -v "${requested_command}" >/dev/null 2>&1; then
  can_exec_directly=1
  if [ "${requested_command}" = "toolkit-dylint" ]; then
    if [ -z "${RUSTUP_TOOLCHAIN:-}" ] && command -v rustc >/dev/null 2>&1; then
      host="$(rustc -vV | awk '/^host: / { print $2 }')"
      if [ -n "${host}" ]; then
        export RUSTUP_TOOLCHAIN="toolkit-nightly-${host}"
      fi
    fi
    if ! command -v cargo-dylint >/dev/null 2>&1 && command -v toolkit-install-dylint >/dev/null 2>&1 && command -v rustup >/dev/null 2>&1; then
      toolkit-install-dylint
    fi
    if ! command -v toolkit-dylint-link >/dev/null 2>&1; then
      can_exec_directly=0
    fi
    if ! command -v cargo-dylint >/dev/null 2>&1; then
      can_exec_directly=0
    fi
    if ! command -v rustup >/dev/null 2>&1; then
      can_exec_directly=0
    fi
  fi
fi

if [ "${can_exec_directly}" -eq 1 ]; then
  exec "$@"
fi

if [ -n "${TOOLKIT_ROOT:-}" ] && [ -f "${TOOLKIT_ROOT}/flake.nix" ]; then
  if [ -n "${requested_command}" ] && [[ "${requested_command}" == toolkit-* ]]; then
    if [ "${requested_command}" = "toolkit-dylint" ]; then
      if [ -z "${RUSTUP_TOOLCHAIN:-}" ] && command -v rustc >/dev/null 2>&1; then
        host="$(rustc -vV | awk '/^host: / { print $2 }')"
        if [ -n "${host}" ]; then
          export RUSTUP_TOOLCHAIN="toolkit-nightly-${host}"
        fi
      fi
      if ! command -v cargo-dylint >/dev/null 2>&1; then
        nix shell "${TOOLKIT_ROOT}#toolkit-install-dylint" --command toolkit-install-dylint
      fi
      exec nix shell "${TOOLKIT_ROOT}#toolkit-dylint" "${TOOLKIT_ROOT}#toolkit-dylint-link" "nixpkgs#rustup" --command "$@"
    fi
    exec nix shell "${TOOLKIT_ROOT}#${requested_command}" --command "$@"
  fi
  exec nix develop "${TOOLKIT_ROOT}" --command "$@"
fi

exec nix develop --command "$@"
