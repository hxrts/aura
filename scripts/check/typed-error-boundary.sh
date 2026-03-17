#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Temporary exemptions (owner: architecture, doc: work/ownership.md)
allowlist=(
  '^crates/aura-app/src/workflows/access\.rs:'
  '^crates/aura-app/src/workflows/context\.rs:'
  '^crates/aura-app/src/workflows/messaging\.rs:'
  '^crates/aura-app/src/workflows/moderation\.rs:'
  '^crates/aura-app/src/workflows/moderator\.rs:'
  '^crates/aura-app/src/workflows/query\.rs:'
  '^crates/aura-app/src/workflows/settings\.rs:'
  '^crates/aura-app/src/workflows/signals\.rs:'
  '^crates/aura-agent/src/handlers/invitation/cache\.rs:'
  '^crates/aura-agent/src/handlers/invitation/channel\.rs:'
  '^crates/aura-agent/src/handlers/invitation/contact\.rs:'
  '^crates/aura-agent/src/handlers/invitation/device_enrollment\.rs:'
  '^crates/aura-agent/src/handlers/invitation/guardian\.rs:'
  '^crates/aura-agent/src/handlers/invitation/validation\.rs:'
  '^crates/aura-terminal/src/tui/context/initialized_app_core\.rs:'
)

fail() {
  echo "typed-error-boundary: $*" >&2
  exit 1
}

# Thin inventory check: parity-critical workflow/runtime/interface paths should
# not wrap primary failures in string formatting. This focuses on structured
# error constructors in the ownership-critical surfaces first.

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  [[ -z "$match" ]] && continue

  allowed=0
  for pattern in "${allowlist[@]}"; do
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      legacy_exemptions=$((legacy_exemptions + 1))
      break
    fi
  done

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
