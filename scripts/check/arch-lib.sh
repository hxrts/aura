#!/usr/bin/env bash
# Shared utilities for arch-* check scripts.
#
# Source this file; do not run directly.
#   source "$(dirname "$0")/arch-lib.sh"

set -euo pipefail
_ARCH_LIB_LOADED=1

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[1]:-${BASH_SOURCE[0]}}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_ROOT"

# Styling
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

# State
VIOLATIONS=0
VIOLATION_DETAILS=()

# Output helpers
printc()    { printf '%b\n' "$1"; }
violation() { ((VIOLATIONS++)) || true; VIOLATION_DETAILS+=("$1"); printc "${RED}✖${NC} $1"; }
info()      { printc "${BLUE}•${NC} $1"; }
section()   { printc "\n${BOLD}${CYAN}$1${NC}"; }
verbose()   { ${VERBOSE:-false} && printc "${BLUE}  ↳${NC} $1" || true; }
hint()      { printc "    ${YELLOW}Fix:${NC} $1"; }

# Allowlists
ALLOW_EFFECTS="crates/aura-effects/src/"
ALLOW_MACROS="crates/aura-macros/src/"
ALLOW_RUNTIME="crates/aura-agent/src/runtime/|crates/aura-agent/src/runtime_bridge_impl.rs|crates/aura-agent/src/builder/"
ALLOW_SIMULATOR="crates/aura-simulator/src/"
ALLOW_CLI="crates/aura-terminal/src/main.rs"
ALLOW_TUI_BOOTSTRAP="crates/aura-terminal/src/handlers/tui.rs"
ALLOW_TUI_INFRA="crates/aura-terminal/src/tui/fullscreen_stdio.rs"
ALLOW_TESTS="crates/aura-testkit/|/tests/|/testing/|/examples/|benches/"
ALLOW_HARNESS="crates/aura-harness/src/"
ALLOW_APP_NATIVE="crates/aura-app/src/core/app.rs|crates/aura-app/src/core/signal_sync.rs"
ALLOW_CRYPTO="crates/aura-core/src/crypto/|crates/aura-core/src/types/authority.rs|crates/aura-effects/src/|crates/aura-testkit/|/tests/|_test\\.rs"
ALLOW_RANDOM="crates/aura-effects/src/|crates/aura-testkit/|/tests/|_test\\.rs"

# Layer utilities
layer_of() {
  case "$1" in
    aura-core) echo 1 ;;
    aura-journal|aura-authorization|aura-signature|aura-store|aura-transport|aura-mpst|aura-macros) echo 2 ;;
    aura-effects|aura-composition) echo 3 ;;
    aura-protocol|aura-guards|aura-consensus|aura-amp|aura-anti-entropy) echo 4 ;;
    aura-authentication|aura-chat|aura-invitation|aura-recovery|aura-relational|aura-rendezvous|aura-sync|aura-app|aura-social) echo 5 ;;
    aura-agent|aura-simulator) echo 6 ;;
    aura-terminal) echo 7 ;;
    aura-testkit|aura-quint) echo 8 ;;
    *) echo 0 ;;
  esac
}

LAYER_FILTERS=()

layer_filter_matches() {
  local layer="$1"
  [[ ${#LAYER_FILTERS[@]} -eq 0 ]] && return 0
  for lf in "${LAYER_FILTERS[@]}"; do [[ "$layer" == "$lf" ]] && return 0; done
  return 1
}

sort_by_layer() {
  while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    local crate layer
    crate=$(echo "${entry%%:*}" | cut -d/ -f2)
    layer=$(layer_of "$crate")
    [[ "$layer" == "0" ]] && layer=99
    printf "%02d:%s\n" "$layer" "$entry"
  done | sort -t: -k1,1n -k2,2
}

# Filtering helpers
filter_allow() {
  local input="$1" extra="${2:-}"
  local result
  result=$(echo "$input" \
    | grep -v "$ALLOW_EFFECTS" \
    | grep -v "$ALLOW_SIMULATOR" \
    | grep -Ev "$ALLOW_TESTS" \
    | grep -v "///" || true)
  [[ -n "$extra" ]] && result=$(echo "$result" | grep -Ev "$extra" || true)
  echo "$result"
}

filter_test_modules() {
  local input="$1"
  [[ -z "$input" ]] && return
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    local file="${line%%:*}"
    [[ ! -f "$file" ]] && { echo "$line"; continue; }
    if grep -q "#\[cfg(test)\]" "$file" 2>/dev/null; then
      local linenum content cfg_line
      if [[ "$line" =~ ^[^:]+:[0-9]+: ]]; then
        linenum=$(echo "$line" | cut -d: -f2)
      else
        content="${line#*:}"
        linenum=$(grep -n "$content" "$file" 2>/dev/null | head -1 | cut -d: -f1)
      fi
      cfg_line=$(grep -n "#\[cfg(test)\]" "$file" 2>/dev/null | head -1 | cut -d: -f1)
      [[ -n "$linenum" && -n "$cfg_line" && "$linenum" -gt "$cfg_line" ]] && continue
    fi
    echo "$line"
  done <<< "$input"
}

emit_hits() {
  local label="$1" hits="$2"
  [[ -z "$hits" ]] && { info "${label}: none"; return; }

  local sorted idx=1 any=false
  sorted=$(echo "$hits" | sort_by_layer)
  while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    local layer="${entry:0:2}" content="${entry:3}"
    [[ "$layer" == "99" ]] && layer="?"
    layer="${layer#0}"
    layer_filter_matches "$layer" || continue
    any=true
    violation "[L${layer}] ${label} [${idx}]: ${content}"
    ((idx++))
  done <<< "$sorted"
  $any || info "${label}: none (filtered)"
}

check_cargo() {
  command -v cargo >/dev/null 2>&1 && return 0
  [[ -x "$HOME/.cargo/bin/cargo" ]] && { export PATH="$HOME/.cargo/bin:$PATH"; return 0; }
  return 1
}

# Summary helper — call at end of arch.sh orchestrator
arch_summary() {
  section "Summary"
  if [[ $VIOLATIONS -eq 0 ]]; then
    printc "${GREEN}✔ No violations${NC}"
  else
    printc "${RED}✖ $VIOLATIONS violation(s)${NC}"
    if ${VERBOSE:-false} && [[ ${#VIOLATION_DETAILS[@]} -gt 0 ]]; then
      printc "\n${BOLD}Violation details:${NC}"
      for d in "${VIOLATION_DETAILS[@]}"; do echo "  - $d"; done
    fi
  fi
  [[ $VIOLATIONS -eq 0 ]]
}
