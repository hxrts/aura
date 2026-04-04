#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# Aura Architectural Compliance Checker
# ═══════════════════════════════════════════════════════════════════════════════
#
# Enforcement split:
# - `just lint-arch-syntax` owns grep-heavy syntax/policy checks that can be
#   enforced more precisely by repo-local Rust-native lints.
# - visibility, constructors, proc macros, and compile-fail tests own
#   API-boundary rules that should fail at compile time.
# - this script keeps the checks that genuinely need workspace topology,
#   docs/governance context, git diff context, or semantic/integration
#   interpretation.

set -euo pipefail

# ───────────────────────────────────────────────────────────────────────────────
# Paths
# ───────────────────────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_ROOT"

# ───────────────────────────────────────────────────────────────────────────────
# Styling
# ───────────────────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

# ───────────────────────────────────────────────────────────────────────────────
# Usage
# ───────────────────────────────────────────────────────────────────────────────
usage() {
  cat <<'EOF'
Aura Architectural Compliance Checker

Usage: scripts/check/arch.sh [OPTIONS]

Options (run all when none given):
  --layers         Retained layer purity/topology checks; syntax allow-policy delegates to lint path
  --deps           Dependency direction checks
  --effects        Retained effect integration/governance checks; syntax escape hatches delegate to lint path
  --invariants     ARCHITECTURE.md invariant section validation
  --todos          Incomplete code markers
  --crypto         Retained crypto lane with delegation note to Rust-native linting
  --concurrency    Retained concurrency lane with delegation note to Rust-native linting
  --reactive       TUI reactive data model
  --ceremonies     Ceremony completion must commit facts
  --ui             UI boundary checks
  --workflows      aura-app workflow hygiene
  --serialization  Retained serialization/integration checks; syntax policy delegates to lint path
  --style          Retained repo-hygiene/style checks; syntax policy delegates to lint path
  --test-seeds     Test seed uniqueness checks
  --layer N[,M...] Filter to specific layers (1-8)
  --quick          Skip slow checks (todos, placeholders)
  -v, --verbose    Show more detail
  -h, --help       Show this help

Primary split:
  - `just lint-arch-syntax`: syntax/policy rules that do not need repo-wide
    integration context
  - compile-fail/API enforcement: visibility, constructors, macros, sealed
    traits, and trybuild boundaries
  - `scripts/check/arch.sh`: topology, docs/governance, semantic integration,
    workflow traceability, and repo-hygiene interpretation
EOF
}

# ───────────────────────────────────────────────────────────────────────────────
# Flag Parsing
# ───────────────────────────────────────────────────────────────────────────────
RUN_ALL=true VERBOSE=false RUN_QUICK=false
RUN_LAYERS=false RUN_DEPS=false RUN_EFFECTS=false
RUN_INVARIANTS=false RUN_TODOS=false RUN_CRYPTO=false
RUN_CONCURRENCY=false RUN_REACTIVE=false RUN_CEREMONIES=false RUN_UI=false RUN_WORKFLOWS=false
RUN_SERIALIZATION=false RUN_STYLE=false RUN_TEST_SEEDS=false
LAYER_FILTERS=()

while [[ $# -gt 0 ]]; do
  case $1 in
    --layers)        RUN_ALL=false; RUN_LAYERS=true ;;
    --deps)          RUN_ALL=false; RUN_DEPS=true ;;
    --effects)       RUN_ALL=false; RUN_EFFECTS=true ;;
    --invariants)    RUN_ALL=false; RUN_INVARIANTS=true ;;
    --todos)         RUN_ALL=false; RUN_TODOS=true ;;
    --crypto)        RUN_ALL=false; RUN_CRYPTO=true ;;
    --concurrency)   RUN_ALL=false; RUN_CONCURRENCY=true ;;
    --reactive)      RUN_ALL=false; RUN_REACTIVE=true ;;
    --ceremonies)    RUN_ALL=false; RUN_CEREMONIES=true ;;
    --ui)            RUN_ALL=false; RUN_UI=true ;;
    --workflows)     RUN_ALL=false; RUN_WORKFLOWS=true ;;
    --serialization) RUN_ALL=false; RUN_SERIALIZATION=true ;;
    --style)         RUN_ALL=false; RUN_STYLE=true ;;
    --test-seeds)    RUN_ALL=false; RUN_TEST_SEEDS=true ;;
    --layer)
      [[ -z "${2-}" ]] && { echo "--layer requires N"; exit 1; }
      IFS=',' read -ra layers <<< "$2"
      LAYER_FILTERS+=("${layers[@]}")
      shift ;;
    --quick)    RUN_QUICK=true ;;
    -v|--verbose) VERBOSE=true ;;
    -h|--help)  usage; exit 0 ;;
    *)          echo "Unknown: $1"; usage; exit 1 ;;
  esac
  shift
done

# Quick mode enables most checks except slow ones
if $RUN_QUICK && $RUN_ALL; then
  RUN_ALL=false
  RUN_LAYERS=true RUN_DEPS=true RUN_EFFECTS=true
  RUN_INVARIANTS=true RUN_CRYPTO=true RUN_CONCURRENCY=true
  RUN_REACTIVE=true RUN_CEREMONIES=true RUN_SERIALIZATION=true RUN_STYLE=true RUN_WORKFLOWS=true
  RUN_TEST_SEEDS=true
  RUN_TODOS=false  # Skip in quick mode
fi

# ───────────────────────────────────────────────────────────────────────────────
# Allowlists (paths that legitimately need impure/direct operations)
# ───────────────────────────────────────────────────────────────────────────────
# Layer 3: Infrastructure effect implementations
ALLOW_EFFECTS="crates/aura-effects/src/"
# Layer 2: Proc-macros run at compile time and need std::fs
ALLOW_MACROS="crates/aura-macros/src/"
# Layer 6: Runtime assembly
ALLOW_RUNTIME="crates/aura-agent/src/runtime/|crates/aura-agent/src/runtime_bridge_impl.rs|crates/aura-agent/src/builder/"
# Layer 6/8: Simulator
ALLOW_SIMULATOR="crates/aura-simulator/src/"
# Layer 7: CLI entry and TUI infrastructure
ALLOW_CLI="crates/aura-terminal/src/main.rs"
ALLOW_TUI_BOOTSTRAP="crates/aura-terminal/src/handlers/tui.rs"
ALLOW_TUI_INFRA="crates/aura-terminal/src/tui/fullscreen_stdio.rs"
# Layer 8: Test infrastructure
ALLOW_TESTS="crates/aura-testkit/|/tests/|/testing/|/examples/|benches/"
# Layer 8: Harness tooling/runtime glue
ALLOW_HARNESS="crates/aura-harness/src/"
# Layer 5: App native-only code (cfg-gated)
ALLOW_APP_NATIVE="crates/aura-app/src/core/app.rs|crates/aura-app/src/core/signal_sync.rs"
# Crypto: Direct library usage
ALLOW_CRYPTO="crates/aura-core/src/crypto/|crates/aura-core/src/types/authority.rs|crates/aura-effects/src/|crates/aura-testkit/|/tests/|_test\\.rs"
ALLOW_RANDOM="crates/aura-effects/src/|crates/aura-testkit/|/tests/|_test\\.rs"

# ───────────────────────────────────────────────────────────────────────────────
# State
# ───────────────────────────────────────────────────────────────────────────────
VIOLATIONS=0
VIOLATION_DETAILS=()

# ───────────────────────────────────────────────────────────────────────────────
# Output Helpers
# ───────────────────────────────────────────────────────────────────────────────
printc()    { printf '%b\n' "$1"; }
violation() { ((VIOLATIONS++)) || true; VIOLATION_DETAILS+=("$1"); printc "${RED}✖${NC} $1"; }
info()      { printc "${BLUE}•${NC} $1"; }
section()   { printc "\n${BOLD}${CYAN}$1${NC}"; }
verbose()   { $VERBOSE && printc "${BLUE}  ↳${NC} $1" || true; }
hint()      { printc "    ${YELLOW}Fix:${NC} $1"; }

# ───────────────────────────────────────────────────────────────────────────────
# Layer Utilities
# ───────────────────────────────────────────────────────────────────────────────
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

layer_filter_matches() {
  local layer="$1"
  [[ ${#LAYER_FILTERS[@]} -eq 0 ]] && return 0
  for lf in "${LAYER_FILTERS[@]}"; do [[ "$layer" == "$lf" ]] && return 0; done
  return 1
}

# Sort hits by layer (L1→L8), output format: "NN:original_line"
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

# ───────────────────────────────────────────────────────────────────────────────
# Filtering Helpers
# ───────────────────────────────────────────────────────────────────────────────
# Filter out common allowlisted paths
filter_allow() {
  local input="$1" extra="${2:-}"
  local result
  result=$(echo "$input" \
    | grep -v "$ALLOW_EFFECTS" \
    | grep -v "$ALLOW_SIMULATOR" \
    | grep -Ev "$ALLOW_TESTS" \
    | grep -v "///" || true)  # Skip doc comments
  [[ -n "$extra" ]] && result=$(echo "$result" | grep -Ev "$extra" || true)
  echo "$result"
}

# Filter lines that are inside #[cfg(test)] modules
filter_test_modules() {
  local input="$1"
  [[ -z "$input" ]] && return
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    local file="${line%%:*}"
    [[ ! -f "$file" ]] && { echo "$line"; continue; }
    # Check if file has #[cfg(test)] and if this line is after it
    if grep -q "#\[cfg(test)\]" "$file" 2>/dev/null; then
      local linenum content cfg_line
      # Try to extract line number (format: file:num:content or file:content)
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

# ───────────────────────────────────────────────────────────────────────────────
# Emit Violations with Layer Tags
# ───────────────────────────────────────────────────────────────────────────────
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

# ───────────────────────────────────────────────────────────────────────────────
# Cargo/Toolchain Check
# ───────────────────────────────────────────────────────────────────────────────
check_cargo() {
  command -v cargo >/dev/null 2>&1 && return 0
  [[ -x "$HOME/.cargo/bin/cargo" ]] && { export PATH="$HOME/.cargo/bin:$PATH"; return 0; }
  return 1
}

run_check() {
  local enabled="$1" check_fn="$2"
  if $RUN_ALL || $enabled; then
    "$check_fn"
  fi
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Layer Purity
# ═══════════════════════════════════════════════════════════════════════════════
check_layers() {
  section "Layer purity — aura-core interface-only; impls in aura-effects or domain crates"

  # aura-core should only define traits/types (no impl of Effects)
  if grep -RE "\bimpl\b.*Effects" crates/aura-core/src 2>/dev/null \
    | grep -v "trait" \
    | grep -v "impl<" \
    | grep -v "ScriptedTimeEffects" \
    | grep -v ":///" >/dev/null; then
    violation "aura-core contains effect implementations (should be interface-only)"
  else
    info "aura-core: interface-only (no effect impls)"
  fi

  # Domain crates should not depend on runtime/UI layers
  for crate in aura-authentication aura-app aura-chat aura-invitation aura-recovery aura-relational aura-rendezvous aura-sync; do
    [[ -d "crates/$crate" ]] || continue
    if grep -A20 "^\[dependencies\]" "crates/$crate/Cargo.toml" | grep -E "aura-agent|aura-simulator|aura-terminal" >/dev/null; then
      violation "$crate depends on runtime/UI layers"
    else
      info "$crate: no runtime/UI deps"
    fi
  done

  section "Layer 4 lint policy — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for crate-level allow-attribute enforcement."
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Dependency Direction
# ═══════════════════════════════════════════════════════════════════════════════
check_deps() {
  section "Dependency direction — no upward deps (Lx→Ly where y>x)"

  if check_cargo; then
    if ! command -v jq >/dev/null 2>&1; then
      violation "jq unavailable; dependency direction not checked"
      return
    fi
    local deps clean=true
    deps=$(cargo metadata --format-version 1 --no-deps 2>/dev/null | jq -r '.packages[] | select(.name | startswith("aura-")) | [.name, (.dependencies[] | select(.name | startswith("aura-")) | .name)] | @tsv') || deps=""
    while IFS=$'\t' read -r src dst; do
      [[ -z "$src" ]] && continue
      local src_l=$(layer_of "$src") dst_l=$(layer_of "$dst")
      if [[ "$src_l" -gt 0 && "$dst_l" -gt 0 && "$dst_l" -gt "$src_l" ]]; then
        violation "$src (L$src_l) depends upward on $dst (L$dst_l)"
        clean=false
      fi
    done <<< "$deps"
    $clean && info "Dependency direction: clean"
  else
    violation "cargo unavailable; dependency direction not checked"
  fi

  # Layer 4 firewall
  section "Layer 4 firewall — no deps on L6+"
  local l4_crates=(aura-protocol aura-guards aura-consensus aura-amp aura-anti-entropy)
  local blocked="aura-agent|aura-simulator|aura-app|aura-terminal|aura-testkit"
  for crate in "${l4_crates[@]}"; do
    [[ -f "crates/$crate/Cargo.toml" ]] || continue
    # Only match actual dependency lines (exclude comments)
    if rg "^[^#]*($blocked)" "crates/$crate/Cargo.toml" >/dev/null 2>&1; then
      violation "$crate depends on L6+ — forbidden"
    else
      info "$crate: firewall clean"
    fi
  done
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Effects System
# ═══════════════════════════════════════════════════════════════════════════════
check_effects() {
  section "Effects syntax and escape hatches — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for effect placement, runtime coupling, and impure/time/random checks."

  section "VM bridge discipline — bridge state and ownership stay centralized"

  local ad_hoc_vm_bridge_hits unreviewed_envelope_mode_hits
  ad_hoc_vm_bridge_hits=$(rg --no-heading -n "Mutex<.*VmBridgePendingSend|Mutex<.*VmBridgeBlockedEdge|Mutex<.*VmBridgeSchedulerSignals|VecDeque<.*VmBridgePendingSend|VecDeque<.*VmBridgeBlockedEdge|VecDeque<.*VmBridgeSchedulerSignals" \
    crates/aura-agent/src crates/aura-testkit/src -g "*.rs" \
    | grep -v "crates/aura-agent/src/runtime/subsystems/vm_bridge.rs" \
    | grep -v "crates/aura-testkit/src/stateful_effects/vm_bridge.rs" || true)
  ad_hoc_vm_bridge_hits=$(filter_test_modules "$ad_hoc_vm_bridge_hits")
  emit_hits "Ad hoc VM bridge queue/state storage outside VmBridgeEffects implementations" "$ad_hoc_vm_bridge_hits"

  unreviewed_envelope_mode_hits=$(rg --no-heading -n "ThreadedVM::with_workers|AuraVmRuntimeMode::ThreadedReplayDeterministic|AuraVmRuntimeMode::ThreadedEnvelopeBounded" \
    crates/aura-agent/src -g "*.rs" \
    | grep -v "crates/aura-agent/src/runtime/choreo_engine.rs" \
    | grep -v "crates/aura-agent/src/runtime/vm_hardening.rs" || true)
  unreviewed_envelope_mode_hits=$(filter_test_modules "$unreviewed_envelope_mode_hits")
  emit_hits "Unreviewed threaded or envelope runtime selection outside hardening/engine paths" "$unreviewed_envelope_mode_hits"

  section "OTA scope model — no network-wide authoritative cutover in code"

  local ota_global_cutover_hits
  ota_global_cutover_hits=$(rg --no-heading -n "GlobalNetwork|NetworkWide|network-wide authoritative cutover|global cutover|whole Aura network.*cutover" \
    crates/aura-maintenance/src \
    crates/aura-sync/src/services \
    crates/aura-agent/src/runtime/services/ota_manager.rs \
    -g "*.rs" || true)
  ota_global_cutover_hits=$(filter_test_modules "$ota_global_cutover_hits")
  emit_hits "OTA code assumes a network-wide authoritative cutover model" "$ota_global_cutover_hits"

  section "Impure/time/random syntax — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for std::fs/std::net/runtime/time/random/sleep checks."

  section "Conformance envelope registry — classify every effect envelope kind"

  local registry_file classified_kinds duplicate_classified
  registry_file="crates/aura-core/src/conformance.rs"
  if [[ ! -f "$registry_file" ]]; then
    violation "Missing conformance registry file: $registry_file"
    hint "Add AURA_EFFECT_ENVELOPE_CLASSIFICATIONS in aura-core and classify each effect kind."
  else
    classified_kinds=$(rg --no-heading '^\s*\(".*",\s*AuraEnvelopeLawClass::' "$registry_file" \
      | sed -E 's/^[[:space:]]*\("([^"]+)".*/\1/' | sort -u || true)

    if [[ -z "$classified_kinds" ]]; then
      violation "No classified effect envelope kinds found in $registry_file"
      hint "Populate AURA_EFFECT_ENVELOPE_CLASSIFICATIONS with strict/commutative/algebraic entries."
    else
      duplicate_classified=$(rg --no-heading '^\s*\(".*",\s*AuraEnvelopeLawClass::' "$registry_file" \
        | sed -E 's/^[[:space:]]*\("([^"]+)".*/\1/' | sort | uniq -d || true)
      if [[ -n "$duplicate_classified" ]]; then
        violation "Duplicate effect envelope classifications found: $(echo "$duplicate_classified" | tr '\n' ' ' | sed 's/  */ /g')"
      fi

      local telltale_src upstream_kinds missing
      telltale_src=$(find "$HOME/.cargo/registry/src" -type d -path "*telltale-machine-*/src" 2>/dev/null | sort -V | tail -n1 || true)

      if [[ -n "$telltale_src" && -d "$telltale_src" ]]; then
        upstream_kinds=$(rg --no-heading 'effect_kind:\s*"[^"]+"' \
          "$telltale_src/commit_common.rs" \
          "$telltale_src/effect/recording_impl.rs" \
          "$telltale_src/threaded/topology_and_planner.rs" \
          "$telltale_src/engine/topology_and_dispatch.rs" \
          | sed -E 's/.*effect_kind:[[:space:]]*"([^"]+)".*/\1/' | sort -u || true)

        if [[ -n "$upstream_kinds" ]]; then
          missing=$(comm -23 <(echo "$upstream_kinds") <(echo "$classified_kinds") || true)
          if [[ -n "$missing" ]]; then
            violation "Unclassified telltale-machine effect kinds: $(echo "$missing" | tr '\n' ' ' | sed 's/  */ /g')"
            hint "Add missing kinds to AURA_EFFECT_ENVELOPE_CLASSIFICATIONS in crates/aura-core/src/conformance.rs."
          else
            info "Effect envelope registry: all telltale-machine effect kinds are classified"
          fi
        else
          info "Effect envelope registry: telltale-machine effect_kind scan returned no kinds"
        fi
      else
        info "Effect envelope registry: telltale-machine source not found in cargo registry; skipping upstream parity check"
      fi
    fi
  fi

  # aura-sync runtime neutrality
  section "aura-sync runtime neutrality"
  info "Run 'just lint-arch-syntax' for aura-sync runtime-neutrality enforcement."

  # ─── Simulation control surfaces ───
  section "Simulation surfaces — inject via effects"

  info "Run 'just lint-arch-syntax' for simulation-surface syntax guardrails."

  # ─── Pure interpreter alignment ───
  section "Pure interpreter — migrate to GuardSnapshot + EffectCommand"

  local guard_sync
  guard_sync=$(rg --no-heading "GuardEffectSystem|futures::executor::block_on" crates -g "*.rs" \
    | grep -v "crates/aura-app/src/frontend_primitives/submitted_operation.rs:" \
    | grep -v "crates/aura-terminal/src/tui/semantic_lifecycle.rs:" \
    | grep -v "crates/aura-ui/src/semantic_lifecycle.rs:" \
    | sort -u || true)
  emit_hits "Synchronous guard/effect bridges" "$guard_sync"

  # ─── Identifier determinism ───
  section "Identifier determinism — avoid entropy-consuming IDs"

  info "Run 'just lint-arch-syntax' for entropy-consuming ID and direct randomness checks."
}
# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Invariant Documentation
# ═══════════════════════════════════════════════════════════════════════════════
check_invariants() {
  section "Invariant docs — crate ARCHITECTURE.md must define invariant sections"

  local arch_files
  arch_files=$(find crates -maxdepth 2 -name ARCHITECTURE.md 2>/dev/null | sort)
  [[ -z "$arch_files" ]] && { violation "No crate ARCHITECTURE.md files found"; return; }

  local with_invariants=0 with_detailed=0
  for arch in $arch_files; do
    if rg -q "^## Invariants" "$arch"; then
      ((with_invariants+=1))
      info "Invariants section: $arch"
    fi

    if rg -q "^### Detailed Specifications$|^## Detailed Invariant Specifications$|^### Invariant" "$arch"; then
      ((with_detailed+=1))
      local missing=()
      rg -qi "Enforcement locus:" "$arch" || missing+=("Enforcement locus")
      rg -qi "Failure mode:" "$arch" || missing+=("Failure mode")
      rg -qi "Verification hooks:" "$arch" || missing+=("Verification hooks")
      if [[ ${#missing[@]} -gt 0 ]]; then
        violation "Missing detailed invariant fields [$(IFS=,; echo "${missing[*]}")]: $arch"
      else
        info "Detailed invariant fields: $arch"
      fi
    fi
  done

  if [[ "$with_invariants" -eq 0 ]]; then
    violation "No crate ARCHITECTURE.md includes an Invariants section"
  fi
  if [[ "$with_detailed" -eq 0 ]]; then
    info "No crate has detailed invariant specs yet"
  fi

  return 0
}
# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Crypto Boundaries
# ═══════════════════════════════════════════════════════════════════════════════
check_crypto() {
  section "Crypto boundaries — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for direct crypto/randomness boundary enforcement."
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Concurrency Hygiene
# ═══════════════════════════════════════════════════════════════════════════════
check_concurrency() {
  section "Concurrency — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for block_in_place / block_on / unbounded-channel checks."
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Reactive Data Model
# ═══════════════════════════════════════════════════════════════════════════════
check_reactive() {
  section "Reactive model — signals are source of truth; no domain data in props"

  # Domain data in props
  local domain_in_props
  domain_in_props=$(rg --no-heading -l "// === Domain data" crates/aura-terminal/src/tui/screens -g "*.rs" || true)
  if [[ -n "$domain_in_props" ]]; then
    local count
    count=$(echo "$domain_in_props" | wc -l | tr -d ' ')
    violation "[L7] Domain data in props: $count screen(s)"
    hint "Remove domain fields from Props; subscribe to signals"
  else
    info "Domain data in props: none"
  fi

  # Screens without signal subscriptions
  local screens_dir="crates/aura-terminal/src/tui/screens"
  if [[ -d "$screens_dir" ]]; then
    local missing=""
    for f in $(find "$screens_dir" -name "screen.rs" 2>/dev/null); do
      grep -q "subscribe_signal_with_retry\|SIGNAL" "$f" 2>/dev/null || missing+="$f"$'\n'
    done
    emit_hits "Screen without signal subscription" "$missing"
  fi

  verbose "Props: only view state (focus, selection), callbacks, config"

  # ─── Fact commit synchronization ───
  section "Fact commit sync — await view updates after commit"

  # Find files that commit facts but don't await reactive processing
  # Allowlist: tests, background sync operations, fire-and-forget contexts, utility helpers
  local commit_allow="crates/aura-testkit/|/tests/|_test\\.rs|crates/aura-simulator/|crates/aura-sync/|handlers/shared\\.rs"
  local commit_files missing_sync=""

  # Find all files that call commit_generic_fact_bytes
  commit_files=$(rg -l "commit_generic_fact_bytes" crates -g "*.rs" | grep -Ev "$commit_allow" || true)

  for f in $commit_files; do
    [[ -z "$f" ]] && continue
    # Check if the file has synchronization pattern:
    # - await_next_view_update (explicit wait)
    # - fire_and_forget (explicit acknowledgment)
    # - FactCommitResult (using the typesafe wrapper)
    if ! grep -qE "await_next_view_update|fire_and_forget|FactCommitResult" "$f" 2>/dev/null; then
      # Additional check: if it's a service/handler that should sync
      if grep -qE "impl.*Handler|impl.*Service|async fn (accept|create|import|send)" "$f" 2>/dev/null; then
        missing_sync+="$f"$'\n'
      fi
    fi
  done

  if [[ -n "$missing_sync" ]]; then
    emit_hits "Fact commit without view sync" "$missing_sync"
    hint "Add await_next_view_update() after commit, or use FactCommitResult pattern"
  else
    info "Fact commit sync: all commits synchronized"
  fi

}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Ceremony Fact Commits
# ═══════════════════════════════════════════════════════════════════════════════
check_ceremonies() {
  section "Ceremony facts — operations that affect UI must commit facts"

  # Pattern: Ceremonies/operations that complete but don't commit corresponding facts.
  # This catches bugs where ceremonies succeed but UI state isn't updated because
  # the facts that drive signal views were never committed.
  #
  # Key insight: Any operation that:
  # 1. Logs "ceremony complete" / "operation complete" / success messages
  # 2. Returns Ok() from a multi-party operation
  # 3. Affects relational state (guardians, contacts, channels, invitations)
  # Must also commit the corresponding facts to update signal views.
  #
  # Runtime bridge files are the integration points where ceremony results become facts.
  # These files must commit RelationalFacts to update signal views.

  local ceremony_allow="crates/aura-testkit/|/tests/|_test\\.rs|crates/aura-simulator/"
  # Exclude read-only views (they read state, don't commit), app core (orchestration),
  # and state trackers (they track in-memory state, runtime_bridge commits facts)
  local ceremony_exclude="/views/|/core/app\\.rs|ceremony_tracker\\.rs|ceremony_processor"
  local ceremony_files missing_facts=""

  # Find files in runtime_bridge that handle ceremony completion
  # These are the critical integration points where protocol results become facts
  ceremony_files=$(rg -l "ceremony.*complete|GuardianBinding|invitation.*accept" \
    crates/aura-agent/src/runtime_bridge -g "*.rs" \
    | grep -Ev "$ceremony_allow" || true)

  for f in $ceremony_files; do
    [[ -z "$f" ]] && continue
    # Check if the file logs ceremony completion
    if grep -qE "ceremony.*complet|guardian.*accept|Committed.*Binding" "$f" 2>/dev/null; then
      # Verify there's a corresponding fact commit
      if ! grep -qE "commit_relational_facts|RelationalFact::" "$f" 2>/dev/null; then
        missing_facts+="$f -- ceremony completion without fact commit"$'\n'
      fi
    fi
  done

  # Also check handler services that execute ceremonies
  local handler_files
  handler_files=$(rg -l "async fn.*ceremony|execute.*ceremony" \
    crates/aura-agent/src/handlers -g "*.rs" \
    | grep -Ev "$ceremony_allow" || true)

  for f in $handler_files; do
    [[ -z "$f" ]] && continue
    # Handlers that execute ceremonies should either:
    # 1. Commit facts directly, OR
    # 2. Delegate to runtime_bridge which commits facts
    if ! grep -qE "commit_relational_facts|runtime_bridge|RelationalFact::" "$f" 2>/dev/null; then
      if grep -qE "ceremony.*complet|Ok\\(CeremonyResult" "$f" 2>/dev/null; then
        missing_facts+="$f -- ceremony handler without fact commit or delegation"$'\n'
      fi
    fi
  done

  if [[ -n "$missing_facts" ]]; then
    emit_hits "Ceremony without fact commit" "$missing_facts"
    hint "Commit RelationalFact after ceremony completion to update signal views"
  else
    info "Ceremony facts: all ceremony completions commit facts"
  fi
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: UI Boundaries
# ═══════════════════════════════════════════════════════════════════════════════
check_ui() {
  section "UI boundary — aura-terminal uses aura_app::ui facade only"

  # Direct app module access
  local app_access
  app_access=$(rg --no-heading "aura_app::(workflows|signal_defs|views|runtime_bridge|authorization)" crates/aura-terminal/src -g "*.rs" | grep -v "///" | grep -v "//" || true)
  emit_hits "Direct aura_app module access" "$app_access"

  # Direct ViewState access
  local view_access
  view_access=$(rg --no-heading "\\.views\\(" crates/aura-terminal/src -g "*.rs" || true)
  emit_hits "Direct ViewState access" "$view_access"

  # Journal/protocol mutation (demo/ is exempt: it simulates peer agents)
  local journal_hits
  journal_hits=$(rg --no-heading "FactRegistry|FactReducer|RelationalFact|JournalEffects|commit_.*facts|RuntimeBridge::commit" crates/aura-terminal/src -g "*.rs" | grep -v "crates/aura-terminal/src/demo/" || true)
  emit_hits "Direct journal/protocol mutation" "$journal_hits"

  # Forbidden crate usage (allow demo/simulation code which needs direct protocol access)
  local forbidden
  forbidden=$(rg --no-heading "aura_(journal|protocol|consensus|guards|amp|anti_entropy|transport|recovery|sync|invitation|authentication|relational|chat)::" crates/aura-terminal/src -g "*.rs" \
    | grep -v "/demo/" \
    | grep -v "/scenarios/" || true)
  emit_hits "Direct protocol/domain crate usage" "$forbidden"

  # ─── Terminal time ───
  section "Terminal time — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for direct wall-clock usage in aura-terminal."

  # ─── Terminal business logic ───
  section "Terminal business logic — keep in aura_app::workflows"

  # Local domain state
  local domain_state
  domain_state=$(rg --no-heading "HashSet<.*Id>|HashMap<.*Id," crates/aura-terminal/src/handlers -g "*.rs" \
    | grep -v "// temporary" | grep -v "// local cache" | grep -Ev "/tests/" || true)
  emit_hits "Local domain state in handlers" "$domain_state"
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Workflow Hygiene
# ═══════════════════════════════════════════════════════════════════════════════
check_workflows() {
  section "Workflow hygiene — use helpers for runtime, parsing, signals"

  # Direct runtime error strings
  local runtime_str
  runtime_str=$(rg --no-heading "Runtime bridge not available" crates/aura-app/src/workflows -g "*.rs" \
    | grep -v "crates/aura-app/src/workflows/runtime.rs" | grep -v "crates/aura-app/src/workflows/error.rs" | grep -v '\.contains(' || true)
  emit_hits "Direct runtime error strings" "$runtime_str"

  # Direct parsing
  local parse_auth
  parse_auth=$(rg --no-heading "parse::<AuthorityId>" crates/aura-app/src/workflows -g "*.rs" \
    | grep -v "crates/aura-app/src/workflows/parse.rs" || true)
  emit_hits "Direct AuthorityId parsing" "$parse_auth"

  local parse_ctx
  parse_ctx=$(rg --no-heading "parse::<ContextId>" crates/aura-app/src/workflows -g "*.rs" \
    | grep -v "crates/aura-app/src/workflows/parse.rs" || true)
  emit_hits "Direct ContextId parsing" "$parse_ctx"

  # Direct signal access
  local signal_access
  signal_access=$(rg --no-heading "\\.(read|emit)\\(&\\*.*_SIGNAL" crates/aura-app/src/workflows -g "*.rs" \
    | grep -v "crates/aura-app/src/workflows/signals.rs" || true)
  emit_hits "Direct signal access" "$signal_access"

  # Direct init_signals calls
  local init_calls
  init_calls=$(rg --no-heading "init_signals\\(" crates/aura-app/src -g "*.rs" \
    | grep -v "crates/aura-app/src/core/app.rs" \
    | grep -v "crates/aura-app/src/core/app/legacy.rs" \
    | grep -v "init_signals_with_hooks" || true)
  emit_hits "Direct init_signals calls" "$init_calls"

  # Strong command pipeline enforcement in TUI slash command path.
  local legacy_slash_dispatch
  legacy_slash_dispatch=$(rg --no-heading "CommandDispatcher::|CapabilityPolicy::|dispatcher\\.dispatch\\(" \
    crates/aura-terminal/src/tui/callbacks/factories -g "*.rs" || true)
  emit_hits "Forbidden slash command dispatcher usage" "$legacy_slash_dispatch"

  local legacy_slash_parse
  legacy_slash_parse=$(rg --no-heading "parse_command\\(" \
    crates/aura-terminal/src/tui/callbacks/factories -g "*.rs" || true)
  emit_hits "Forbidden slash parse helper usage" "$legacy_slash_parse"

  local legacy_input_parse
  legacy_input_parse=$(rg --no-heading "parse_command\\(" \
    crates/aura-terminal/src/tui/state/handlers/input.rs -g "*.rs" || true)
  emit_hits "Forbidden slash parse helper usage in input handler" "$legacy_input_parse"

  local legacy_dispatch_refs
  legacy_dispatch_refs=$(rg --no-heading "CommandDispatcher|CapabilityPolicy" \
    crates/aura-terminal/src -g "*.rs" \
    | grep -v "crates/aura-terminal/src/tui/effects/dispatcher.rs" \
    | grep -v "crates/aura-terminal/src/tui/effects/mod.rs" || true)
  emit_hits "Forbidden dispatcher references outside dispatcher module" "$legacy_dispatch_refs"

  local legacy_parse_refs
  legacy_parse_refs=$(rg --no-heading "parse_command\\(" \
    crates/aura-terminal/src -g "*.rs" \
    | grep -v "crates/aura-terminal/src/tui/commands.rs" || true)
  emit_hits "Forbidden parse helper references outside commands module" "$legacy_parse_refs"

  local missing_strong=false
  if ! rg -q "workflows::strong_command::execute_planned" \
    crates/aura-terminal/src/tui/callbacks/factories; then
    violation "[L7] Strong command pipeline missing: callbacks/factories must call strong_command::execute_planned"
    missing_strong=true
  fi
  if ! rg -q "strong_resolver\\.plan\\(" \
    crates/aura-terminal/src/tui/callbacks/factories; then
    violation "[L7] Strong command planning missing: callbacks/factories must plan resolved commands before execution"
    missing_strong=true
  fi
  $missing_strong || info "Strong command pipeline: enforced"

  section "Workflow legibility — typed boundaries and docs traceability"

  # Prefer typed error/result boundaries at workflow surfaces.
  # Existing exceptions are narrow helper parsers/checks slated for later cleanup.
  local string_results
  string_results=$(rg --no-heading "Result<[^>]*,\s*String>" crates/aura-app/src/workflows -g "*.rs" \
    | grep -Ev "crates/aura-app/src/workflows/(authority|budget|chat_commands)\\.rs:" || true)
  emit_hits "Untyped workflow result (Result<_, String>)" "$string_results"

  # Avoid ad-hoc JSON value plumbing in workflow logic.
  local json_value_hits
  json_value_hits=$(rg --no-heading "serde_json::Value" crates/aura-app/src/workflows -g "*.rs" \
    | grep -Ev "crates/aura-app/src/workflows/recovery_cli\\.rs:" || true)
  emit_hits "Stringly JSON workflow surface (serde_json::Value)" "$json_value_hits"

  # New workflow/tui effect surface files must be accompanied by docs touch.
  local diff_range=""
  if git rev-parse --verify HEAD^2 >/dev/null 2>&1; then
    diff_range="HEAD^1...HEAD"
  elif git rev-parse --verify HEAD^ >/dev/null 2>&1; then
    diff_range="HEAD^..HEAD"
  fi

  if [[ -n "$diff_range" ]]; then
    local added_surfaces docs_touch
    added_surfaces=$(git diff --name-status --diff-filter=A "$diff_range" \
      | awk '{print $2}' \
      | grep -E "^crates/aura-app/src/workflows/.*\\.rs$|^crates/aura-terminal/src/tui/effects/.*\\.rs$" || true)
    if [[ -n "$added_surfaces" ]]; then
      docs_touch=$(git diff --name-only "$diff_range" \
        | grep -E "^docs/(116_cli_tui\\.md|804_testing_guide\\.md|807_system_internals_guide\\.md|997_ux_flow_coverage\\.md|999_project_structure\\.md)$" || true)
      if [[ -z "$docs_touch" ]]; then
        emit_hits "New workflow surface without docs update" "$added_surfaces"
        hint "When adding workflow/tui effect surface files, update at least one architecture/testing doc."
      else
        info "Workflow docs traceability: docs updated alongside new surfaces"
      fi
    else
      info "Workflow docs traceability: no new workflow/tui effect surfaces"
    fi
  else
    info "Workflow docs traceability: skipped (insufficient git history)"
  fi
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Serialization
# ═══════════════════════════════════════════════════════════════════════════════
check_serialization() {
  section "Serialization — use DAG-CBOR; no bincode"
  info "Run 'just lint-arch-syntax' for bincode usage and syntax-owned serialization/style checks."

  # Wire protocols without canonical serialization
  local wire_files non_canonical=""
  wire_files=$(find crates -type f \( -name "wire.rs" -o -name "*_wire.rs" \) 2>/dev/null || true)
  for f in $wire_files; do
    if grep -q "serde_json::to_vec\|serde_json::from_slice\|bincode::" "$f" 2>/dev/null; then
      grep -q "aura_core::util::serialization\|crate::util::serialization" "$f" 2>/dev/null || non_canonical+="$f"$'\n'
    fi
  done
  [[ -n "$non_canonical" ]] && emit_hits "Wire protocol without DAG-CBOR" "$non_canonical" || info "Wire protocols: canonical"

  # Facts without versioned serialization
  local facts_files non_versioned=""
  facts_files=$(find crates -path "*/src/facts.rs" -type f 2>/dev/null | grep -v aura-core || true)
  for f in $facts_files; do
    if grep -q "Serialize\|Deserialize" "$f" 2>/dev/null; then
      grep -qE "aura_core::util::serialization|Versioned.*Fact|from_slice|to_vec" "$f" 2>/dev/null || non_versioned+="$f"$'\n'
    fi
  done
  [[ -n "$non_versioned" ]] && emit_hits "Facts without versioned serialization" "$non_versioned" || info "Facts: versioned"

  # ─── Wire protocol types must use DAG-CBOR tests (not serde_json) ───
  section "Wire protocol types — require DAG-CBOR tests"

  # Wire protocol types (protocol.rs in feature crates) define messages
  # sent between devices. Tests must use DAG-CBOR (aura_core::util::serialization)
  # not serde_json, since DAG-CBOR is the runtime format.
  #
  # Bug pattern: tests pass with serde_json but fail at runtime with DAG-CBOR
  # because the formats have different behaviors for missing/extra fields.

  local using_serde_json=""

  # Find protocol.rs files using serde_json instead of DAG-CBOR for tests
  local protocol_files
  protocol_files=$(rg -l "#\[derive.*Serialize.*Deserialize|#\[derive.*Deserialize.*Serialize" crates -g "protocol.rs" \
    | grep -Ev "/tests/|/benches/|/examples/" || true)

  for file in $protocol_files; do
    [[ -z "$file" ]] && continue
    # Check if the file uses serde_json in tests (should use DAG-CBOR instead)
    if grep -qE "serde_json::(to_vec|from_slice|to_string|from_str)" "$file" 2>/dev/null; then
      # Verify it's not also using DAG-CBOR (which would be OK)
      if ! grep -qE "aura_core::util::serialization|serde_ipld_dagcbor" "$file" 2>/dev/null; then
        using_serde_json+="$file -- tests use serde_json but runtime uses DAG-CBOR"$'\n'
      fi
    fi
  done

  if [[ -n "$using_serde_json" ]]; then
    emit_hits "Wire protocol using wrong serialization" "$using_serde_json"
    hint "Replace serde_json with aura_core::util::serialization::{to_vec, from_slice} in tests"
  else
    info "Wire protocol types: serialization format consistent"
  fi
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Runtime Handler Hygiene
# ═══════════════════════════════════════════════════════════════════════════════
check_handler_hygiene() {
  section "Handler hygiene — stateless handlers; no bridge modules"

  # Allow ceremony services that need state for multi-step coordination
  local ceremony_services="ota_activation_service|recovery_service"
  local handler_state
  handler_state=$(rg --no-heading "Arc<.*(RwLock|Mutex)|RwLock<|Mutex<" crates/aura-agent/src/handlers -g "*.rs" \
    | grep -Ev "$ceremony_services" || true)
  emit_hits "Stateful handlers" "$handler_state"

  local bridge_files
  bridge_files=$(rg --files -g "*bridge*.rs" crates/aura-agent/src/handlers 2>/dev/null || true)
  emit_hits "Handler bridge modules" "$bridge_files"
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Rust Style Guide
# ═══════════════════════════════════════════════════════════════════════════════
check_style() {
  section "Rust style — safety and API rules"
  info "Run 'just lint-arch-syntax' for serialized-usize, unit-suffix, and builder-#[must_use] checks."

  # Lonely mod.rs files (directory with only mod.rs should be a single file)
  local lonely_mods=""
  while IFS= read -r modrs; do
    [[ -z "$modrs" ]] && continue
    local dir
    dir=$(dirname "$modrs")
    # Count .rs files in directory (excluding mod.rs itself)
    local sibling_count
    sibling_count=$(find "$dir" -maxdepth 1 -name "*.rs" ! -name "mod.rs" 2>/dev/null | wc -l | tr -d ' ')
    # Count subdirectories that are modules (have mod.rs or are named like modules)
    local subdir_count
    subdir_count=$(find "$dir" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$sibling_count" -eq 0 && "$subdir_count" -eq 0 ]]; then
      lonely_mods+="$modrs"$'\n'
    fi
  done < <(find crates -name "mod.rs" -type f 2>/dev/null)
  emit_hits "Lonely mod.rs (convert to single file)" "$lonely_mods"

  # Empty directories (should be deleted or have .gitkeep)
  local empty_dirs=""
  while IFS= read -r dir; do
    [[ -z "$dir" ]] && continue
    # Skip if it's a git directory or target
    [[ "$dir" == *".git"* || "$dir" == *"target"* ]] && continue
    # Skip gitignored directories
    git check-ignore -q "$dir" 2>/dev/null && continue
    # Check if directory is truly empty (no files, no subdirs)
    local file_count
    file_count=$(find "$dir" -maxdepth 1 -type f 2>/dev/null | wc -l | tr -d ' ')
    local subdir_count
    subdir_count=$(find "$dir" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$file_count" -eq 0 && "$subdir_count" -eq 0 ]]; then
      empty_dirs+="$dir"$'\n'
    fi
  done < <(find crates -type d 2>/dev/null)
  emit_hits "Empty directory (delete or add .gitkeep)" "$empty_dirs"

  info "Style checks complete"
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Test Seed Uniqueness
# ═══════════════════════════════════════════════════════════════════════════════
check_test_seeds() {
  section "Test seed uniqueness — ensure test isolation"

  # Run the dedicated test seed checker script
  if AURA_CHECK_ARCH_MODE=1 bash scripts/check/test-seeds.sh; then
    info "Test seed uniqueness: clean"
  else
    violation "Test seed uniqueness violations detected (see checker output above)"
  fi
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: TODOs and Incomplete Markers
# ═══════════════════════════════════════════════════════════════════════════════
check_todos() {
  # Placeholders
  section "Placeholders — replace nil UUIDs with real derivations"
  local placeholder_hits
  placeholder_hits=$(rg --no-heading -i "uuid::nil\\(\\)|placeholder implementation" crates -g "*.rs" \
    | grep -Ev "/tests/|/benches/|/examples/|crates/aura-simulator/|/scenarios/|/demo/" || true)
  if [[ -n "$placeholder_hits" ]]; then
    local formatted
    formatted=$(echo "$placeholder_hits" | while read -r e; do [[ -n "$e" ]] && echo "$e -- derive real IDs"; done)
    emit_hits "Placeholder ID" "$formatted"
  else
    info "Placeholder IDs: none"
  fi

  # Deterministic algorithm stubs
  section "Deterministic algorithm TODOs"
  local det_hits
  det_hits=$(rg --no-heading -i "deterministic algorithm" crates -g "*.rs" | grep -Ev "/tests/|/benches/|/examples/" || true)
  if [[ -n "$det_hits" ]]; then
    local formatted
    formatted=$(echo "$det_hits" | while read -r e; do [[ -n "$e" ]] && echo "$e -- implement per spec"; done)
    emit_hits "Deterministic algorithm stub" "$formatted"
  else
    info "Deterministic stubs: none"
  fi

  # Temporary contexts
  section "Temporary context fallbacks"
  local temp_hits
  temp_hits=$(rg --no-heading -i "temporary context|temp context" crates -g "*.rs" | grep -Ev "/tests/|/benches/|/examples/" || true)
  if [[ -n "$temp_hits" ]]; then
    local formatted
    formatted=$(echo "$temp_hits" | while read -r e; do [[ -n "$e" ]] && echo "$e -- resolve via journal state"; done)
    emit_hits "Temporary context" "$formatted"
  else
    info "Temporary contexts: none"
  fi

  # TODO/FIXME
  section "TODO/FIXME markers"
  local platform_allow="crates/aura-agent/src/builder/android.rs|crates/aura-agent/src/builder/ios.rs|crates/aura-agent/src/builder/web.rs"
  local tui_allow="Implement channel deletion callback|Implement contact removal callback|Implement invitation revocation callback|Pass actual channel"
  local chaos_allow="tree_chaos.rs.*Re-enable when chaos testing infrastructure is ready"
  local todo_hits
  todo_hits=$(rg --no-heading "TODO|FIXME" crates -g "*.rs" \
    | grep -Ev "/benches/" \
    | grep -Ev "$platform_allow" \
    | grep -Ev "$tui_allow" \
    | grep -Ev "$chaos_allow" || true)
  emit_hits "TODO/FIXME" "$todo_hits"

  # Incomplete markers
  section "Incomplete/WIP markers"
  local incomplete_pattern="in production[^\\n]*(would|should|not)|in a full implementation|stub|not implemented|unimplemented|temporary|workaround|hacky|\\bWIP\\b|\\bTBD\\b|prototype|future work|to be implemented"
  local stub_allow="biscuit_capability_stub|in production this would be the actual|effects/dispatcher.rs.*[Ss]tub|effects/dispatcher.rs.*[Ii]n production"
  local incomplete_hits
  incomplete_hits=$(rg --no-heading -i "$incomplete_pattern" crates -g "*.rs" \
    | grep -Ev "/tests/|/benches/|/examples/|/bin/" \
    | grep -Ev "$stub_allow" \
    | grep -E "//" || true)
  [[ -n "$incomplete_hits" ]] && emit_hits "Incomplete/WIP" "$incomplete_hits" || info "Incomplete markers: none"
}


# ═══════════════════════════════════════════════════════════════════════════════
# Main Execution
# Retained shell-owned checks run here. Lint-owned syntax policy is delegated
# earlier via the per-lane informational messages above.
# ═══════════════════════════════════════════════════════════════════════════════
run_check "$RUN_LAYERS" check_layers
run_check "$RUN_DEPS" check_deps
run_check "$RUN_EFFECTS" check_effects
run_check "$RUN_INVARIANTS" check_invariants
run_check "$RUN_CRYPTO" check_crypto
run_check "$RUN_CONCURRENCY" check_concurrency
run_check "$RUN_REACTIVE" check_reactive
run_check "$RUN_CEREMONIES" check_ceremonies
run_check "$RUN_UI" check_ui
run_check "$RUN_WORKFLOWS" check_workflows
if $RUN_ALL || $RUN_SERIALIZATION; then
  check_serialization
  check_handler_hygiene
fi
run_check "$RUN_STYLE" check_style
run_check "$RUN_TEST_SEEDS" check_test_seeds
run_check "$RUN_TODOS" check_todos


# ═══════════════════════════════════════════════════════════════════════════════
# Summary
# ═══════════════════════════════════════════════════════════════════════════════
section "Summary"
if [[ $VIOLATIONS -eq 0 ]]; then
  printc "${GREEN}✔ No violations${NC}"
else
  printc "${RED}✖ $VIOLATIONS violation(s)${NC}"
  if $VERBOSE && [[ ${#VIOLATION_DETAILS[@]} -gt 0 ]]; then
    printc "\n${BOLD}Violation details:${NC}"
    for d in "${VIOLATION_DETAILS[@]}"; do echo "  - $d"; done
  fi
fi

if [[ $VIOLATIONS -gt 10 ]] && ! $RUN_QUICK; then
  printc "\n${YELLOW}Tip:${NC} Use --quick to skip TODO/placeholder checks"
fi

if [[ $VIOLATIONS -eq 0 ]]; then
  exit 0
fi
exit 1
