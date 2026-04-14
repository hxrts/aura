#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

local_toolkit="$(cd "${repo_root}/.." && pwd)/toolkit"
if [ -d "${local_toolkit}/xtask" ]; then
  export TOOLKIT_ROOT="${local_toolkit}"
fi

requested_command="${1:-}"

setup_toolkit_dylint_tmpdir() {
  local current_tmpdir="${TMPDIR:-}"
  if [ -z "${current_tmpdir}" ]; then
    export TMPDIR="/tmp"
    return
  fi

  case "${current_tmpdir}" in
    "${repo_root}" | "${repo_root}"/*)
      export TMPDIR="/tmp"
      ;;
  esac
}

setup_toolkit_dylint_env() {
  if [ -n "${HOME:-}" ]; then
    export PATH="${HOME}/.cargo/bin:${PATH}"
  fi

  if [ -z "${AURA_TOOLKIT_NIGHTLY_BIN:-}" ] || [ ! -x "${AURA_TOOLKIT_NIGHTLY_BIN}/cargo" ]; then
    return
  fi

  local host
  host="$("${AURA_TOOLKIT_NIGHTLY_BIN}/rustc" -vV | awk '/^host: / { print $2 }')"
  if [ -z "${host}" ]; then
    return
  fi

  local toolchain_name="${RUSTUP_TOOLCHAIN:-toolkit-nightly-${host}}"
  export RUSTUP_TOOLCHAIN="${toolchain_name}"

  local shim_root="${XDG_CACHE_HOME:-${HOME}/.cache}/toolkit/nightly-shims/${host}"
  mkdir -p "${shim_root}"

  cat > "${shim_root}/cargo" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export RUSTUP_TOOLCHAIN="${toolchain_name}"
exec "${AURA_TOOLKIT_NIGHTLY_BIN}/cargo" "\$@"
EOF
  cat > "${shim_root}/rustc" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export RUSTUP_TOOLCHAIN="${toolchain_name}"
exec "${AURA_TOOLKIT_NIGHTLY_BIN}/rustc" "\$@"
EOF
  cat > "${shim_root}/rustdoc" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export RUSTUP_TOOLCHAIN="${toolchain_name}"
exec "${AURA_TOOLKIT_NIGHTLY_BIN}/rustdoc" "\$@"
EOF
  chmod +x "${shim_root}/cargo" "${shim_root}/rustc" "${shim_root}/rustdoc"

  export PATH="${shim_root}:${AURA_TOOLKIT_NIGHTLY_BIN}:${PATH}"
}

if [ "${requested_command}" = "toolkit-dylint" ]; then
  setup_toolkit_dylint_env
  setup_toolkit_dylint_tmpdir
fi

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
      setup_toolkit_dylint_env
      exec nix shell "${TOOLKIT_ROOT}#toolkit-dylint" "${TOOLKIT_ROOT}#toolkit-dylint-link" --command "$@"
    fi
    exec nix shell "${TOOLKIT_ROOT}#${requested_command}" --command "$@"
  fi
  exec nix develop "${TOOLKIT_ROOT}" --command "$@"
fi

exec nix develop --command "$@"
