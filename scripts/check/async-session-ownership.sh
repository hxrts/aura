#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/async-session-ownership.allowlist"

fail() {
  echo "async-session-ownership: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  allowed=0
  while IFS= read -r pattern; do
    [[ -z "$pattern" || "$pattern" =~ ^# ]] && continue
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      legacy_exemptions=$((legacy_exemptions + 1))
      break
    fi
  done < "$allowlist_file"

  if (( allowed == 0 )); then
    violations+=("$match")
  fi
done < <(
  rg -n \
    -e 'open_manifest_vm_session_admitted' \
    -e 'inject_vm_receive' \
    -e 'effects\.start_session\(' \
    -e 'self\.effects\.start_session\(' \
    -e 'effects\.end_session\(' \
    -e 'self\.effects\.end_session\(' \
    crates/aura-agent/src/handlers crates/aura-agent/src/runtime/services -g '*.rs' \
    | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "direct VM/session mutation bypasses runtime/session_ingress.rs"
fi

echo "async session ownership: clean (${legacy_exemptions} temporary exemptions)"
