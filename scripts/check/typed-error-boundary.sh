#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/typed-error-boundary.allowlist"

fail() {
  echo "typed-error-boundary: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

# Thin inventory check: parity-critical workflow/runtime/interface paths should
# not wrap primary failures in string formatting. This focuses on structured
# error constructors in the ownership-critical surfaces first.

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  [[ -z "$match" ]] && continue

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
  {
    rg -n \
      -e 'AuraError::(internal|terminal|permission_denied|not_found|invalid_input)\(format!' \
      -e '(?:crate::core::)?AgentError::(internal|runtime|effects|invalid|config)\(format!' \
      -e 'SemanticOperationError::[A-Za-z_]+\(\s*format!' \
      crates/aura-app/src/workflows \
      crates/aura-agent/src/handlers/invitation \
      crates/aura-agent/src/runtime_bridge \
      crates/aura-terminal/src/tui \
      crates/aura-web/src \
      -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "parity-critical workflow/runtime paths still use stringly primary error construction"
fi

echo "typed error boundary: clean (${legacy_exemptions} temporary exemptions)"
