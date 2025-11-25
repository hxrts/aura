#!/usr/bin/env bash
# Aura Architectural Compliance Checker (trimmed and opinionated)
set -euo pipefail

# Styling
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

usage() {
  cat <<'EOF'
Aura Architectural Compliance Checker

Usage: scripts/arch-check.sh [OPTIONS]

Options (run all when none given):
  --layers         Layer boundary and purity checks
  --deps           Dependency direction checks
  --effects        Effect placement and handler sanity
  --guards         Guard-chain bypass heuristics
  --invariants     INVARIANTS.md schema validation
  --todos          Incomplete code markers
  --registration   Handler composition vs direct instantiation
  -h, --help       Show this help
EOF
}

RUN_ALL=true
RUN_LAYERS=false
RUN_DEPS=false
RUN_EFFECTS=false
RUN_GUARDS=false
RUN_INVARIANTS=false
RUN_TODOS=false
RUN_REG=false

while [[ $# -gt 0 ]]; do
  case $1 in
    --layers) RUN_ALL=false; RUN_LAYERS=true ;;
    --deps) RUN_ALL=false; RUN_DEPS=true ;;
    --effects) RUN_ALL=false; RUN_EFFECTS=true ;;
    --guards) RUN_ALL=false; RUN_GUARDS=true ;;
    --invariants) RUN_ALL=false; RUN_INVARIANTS=true ;;
    --todos) RUN_ALL=false; RUN_TODOS=true ;;
    --registration) RUN_ALL=false; RUN_REG=true ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1"; usage; exit 1 ;;
  esac
  shift
done

VIOLATIONS=0; WARNINGS=0
VIOLATION_DETAILS=(); WARNING_DETAILS=()

violation() { VIOLATIONS=$((VIOLATIONS+1)); VIOLATION_DETAILS+=("$1"); echo -e "${RED}✖${NC} $1"; }
warning() { WARNINGS=$((WARNINGS+1)); WARNING_DETAILS+=("$1"); echo -e "${YELLOW}•${NC} $1"; }
info() { echo -e "${BLUE}•${NC} $1"; }

section() { echo -e "\n${BOLD}${CYAN}$1${NC}"; }

check_cargo() { command -v cargo >/dev/null 2>&1; }

layer_of() {
  case "$1" in
    aura-core) echo 1 ;;
    aura-journal|aura-wot|aura-verify|aura-store|aura-transport|aura-mpst|aura-macros) echo 2 ;;
    aura-effects|aura-composition) echo 3 ;;
    aura-protocol) echo 4 ;;
    aura-authenticate|aura-chat|aura-invitation|aura-recovery|aura-relational|aura-rendezvous|aura-sync) echo 5 ;;
    aura-agent|aura-simulator) echo 6 ;;
    aura-cli) echo 7 ;;
    aura-testkit|aura-quint) echo 8 ;;
    *) echo 0 ;;
  esac
}

if [ "$RUN_ALL" = true ] || [ "$RUN_LAYERS" = true ]; then
  section "Layer purity"
  # aura-core should only define traits/types (no impl of Effects)
  if grep -R "impl.*Effects" crates/aura-core/src 2>/dev/null | grep -v "trait" | grep -v "impl<T" >/dev/null; then
    violation "aura-core contains effect implementations (should be interface-only)"
  else
    info "aura-core: interface-only (no effect impls)"
  fi

  # Domain crates should not depend on runtime/UI layers
  for crate in aura-authenticate aura-chat aura-invitation aura-recovery aura-relational aura-rendezvous aura-sync; do
    if [ -d "crates/$crate" ]; then
      if grep -A20 "^\[dependencies\]" crates/$crate/Cargo.toml | grep -E "aura-agent|aura-simulator|aura-cli" >/dev/null; then
        violation "$crate depends on runtime/UI layers"
      else
        info "$crate: no runtime/UI deps"
      fi
    fi
  done
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_DEPS" = true ]; then
  section "Dependency direction"
  if check_cargo; then
    deps=$(cargo metadata --format-version 1 --no-deps 2>/dev/null | jq -r '.packages[] | select(.name | startswith("aura-")) | [.name, (.dependencies[] | select(.name | startswith("aura-")) | .name)] | @tsv') || deps=""
    clean=true
    while IFS=$'\t' read -r src dst; do
      [ -z "$src" ] && continue
      src_layer=$(layer_of "$src"); dst_layer=$(layer_of "$dst")
      if [ "$src_layer" -gt 0 ] && [ "$dst_layer" -gt 0 ] && [ "$dst_layer" -gt "$src_layer" ]; then
        violation "$src (L$src_layer) depends upward on $dst (L$dst_layer)"
        clean=false
      fi
    done <<< "$deps"
    if [ "$clean" = true ]; then info "Dependency direction: clean"; fi
  else
    warning "cargo unavailable; dependency direction not checked"
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_EFFECTS" = true ]; then
  section "Effects"
  # Infrastructure effect traits must live in aura-core
  infra_traits="CryptoEffects|NetworkEffects|StorageEffects|PhysicalTimeEffects|LogicalClockEffects|OrderClockEffects|TimeAttestationEffects|RandomEffects|ConsoleEffects|ConfigurationEffects"
  infra_defs=$(find crates/ -name "*.rs" -not -path "*/aura-core/*" -exec grep -El "pub trait ($infra_traits)" {} + 2>/dev/null || true)
  if [ -n "$infra_defs" ]; then
    violation "Infrastructure effect traits defined outside aura-core:" 
    echo "$infra_defs"
  else
    info "Infra effect traits defined only in aura-core"
  fi

  # aura-effects should stay stateless
  if grep -R "Arc<Mutex\|Arc<RwLock\|Rc<RefCell" crates/aura-effects/src 2>/dev/null | grep -v "test" >/dev/null; then
    violation "aura-effects contains stateful constructs (should be stateless handlers)"
  fi

  # Guard for mocks in aura-effects
  if grep -R "Mock.*Handler\|InMemory.*Handler" crates/aura-effects/src 2>/dev/null | grep -v "test" >/dev/null; then
    violation "Mock/test handlers found in aura-effects (should be in aura-testkit)"
  fi

  # Check for infrastructure effect implementations outside aura-effects
  # Only flag concrete impl blocks (not type bounds) of infra effects outside aura-effects/testkit
  infra_impls=$(rg --no-heading --glob "*.rs" "impl\\s+[^\n{}]*for[^\n{}]*(CryptoEffects|NetworkEffects|StorageEffects|PhysicalTimeEffects|LogicalClockEffects|OrderClockEffects|TimeAttestationEffects|RandomEffects|ConsoleEffects|ConfigurationEffects)" crates \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-core/" \
    | grep -v "tests/" || true)
  if [ -n "$infra_impls" ]; then
    warning "Infrastructure effects implemented outside aura-effects:"
    echo "$infra_impls"
  else
    info "Infrastructure effect impls confined to aura-effects/testkit"
  fi

  # Check for application effects in aura-effects
  app_effects="JournalEffects|AuthorityEffects|FlowBudgetEffects|LeakageEffects|AuthorizationEffects|RelationalContextEffects|GuardianEffects"
  app_impls=$(grep -R "impl.*\($app_effects\)" crates/aura-effects/src 2>/dev/null | grep -v "test" || true)
  if [ -n "$app_impls" ]; then
    violation "Application effects implemented in aura-effects (should be in domain crates)"
  else
    info "No application effects implemented in aura-effects"
  fi

  # Check for direct OS operations in domain handlers
  domain_crates="aura-journal|aura-wot|aura-verify|aura-store|aura-transport|aura-authenticate|aura-recovery|aura-relational"
  os_violations=$(find crates/ -path "*/src/*" -name "*.rs" | grep -E "($domain_crates)" | xargs grep -l "std::fs::\|SystemTime::now\|thread_rng()" 2>/dev/null | grep -v "test" || true)
  if [ -n "$os_violations" ]; then
    warning "Direct OS operations in domain crates (should use effect injection):"
    echo "$os_violations"
  else
    info "No direct OS ops in domain crates"
  fi

  section "Impure functions"
  # Strict flag for direct wall-clock/random usage outside allowed areas
  impure_pattern="SystemTime::now|Instant::now|thread_rng\\(|rand::thread_rng|chrono::Utc::now|chrono::Local::now|rand::rngs::OsRng|rand::random"
  impure_hits=$(rg --no-heading "$impure_pattern" crates -g "*.rs" || true)
  # Allowlist: effect implementations, testkit mocks, simulator handlers, runtime assembly
  filtered_impure=$(echo "$impure_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-agent/src/runtime/" \
    | grep -v "tests/performance_regression.rs" || true)
  if [ -n "$filtered_impure" ]; then
    violation "Impure functions detected outside effect implementations/testkit/runtime assembly:"
    echo "$filtered_impure"
  else
    info "Impure function usage confined to effect implementations/testkit/runtime"
  fi

  # Check for direct sleeps (should use effect-injected time, especially for simulator determinism)
  sleep_pattern="std::thread::sleep|tokio::time::sleep|async_std::task::sleep"
  sleep_hits=$(rg --no-heading "$sleep_pattern" crates -g "*.rs" || true)
  filtered_sleep=$(echo "$sleep_hits" \
    | grep -v "crates/aura-effects/src/time.rs" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "/tests/" \
    | grep -v "benches/" || true)
  if [ -n "$filtered_sleep" ]; then
    warning "Direct sleeps detected (should be effect-injected/simulator-controlled):"
    echo "$filtered_sleep"
  else
    info "No direct sleep usage outside allowed handlers/simulator"
  fi

  section "Simulation control surfaces"
  sim_patterns="rand::random|rand::thread_rng|rand::rngs::OsRng|RngCore::fill_bytes|std::io::stdin|read_line\\(|std::thread::spawn"
  sim_hits=$(rg --no-heading "$sim_patterns" crates -g "*.rs" || true)
  filtered_sim=$(echo "$sim_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-agent/src/runtime/" \
    | grep -v "/tests/" || true)
  if [ -n "$filtered_sim" ]; then
    warning "Potential non-injected randomness/IO/spawn (should be simulator-controllable; see docs/806_simulation_guide.md and .claude/skills/common_patterns.md):"
    echo "$filtered_sim"
  else
    info "Randomness/IO/spawn hooks appear confined to injectable layers"
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_GUARDS" = true ]; then
  section "Guard chain"
  transport_sends=$(rg --no-heading "TransportEffects::(send|open_channel)" crates -g "*.rs" || true)
  guard_allowlist="crates/aura-protocol/src/guards|crates/aura-protocol/src/handlers/sessions|tests/|crates/aura-testkit/"
  bypass_hits=$(echo "$transport_sends" | grep -Ev "$guard_allowlist" || true)
  if [ -n "$bypass_hits" ]; then
    warning "Potential guard-chain bypass (TransportEffects send/open outside guard modules):"
    echo "$bypass_hits"
  else
    info "Transport usage confined to guard chain modules"
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_INVARIANTS" = true ]; then
  section "Invariant docs"
  invariant_files=$(find crates -name INVARIANTS.md 2>/dev/null | sort)
  if [ -z "$invariant_files" ]; then
    warning "No INVARIANTS.md files found"
  else
    for inv in $invariant_files; do
      missing=()
      for heading in "Invariant Name" "Enforcement Locus" "Failure Mode" "Detection Method"; do
        if ! grep -q "$heading" "$inv"; then
          missing+=("$heading")
        fi
      done
      if [ ${#missing[@]} -gt 0 ]; then
        violation "$inv missing required sections: ${missing[*]}"
      else
        info "$inv: schema OK"
      fi
    done
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_REG" = true ]; then
  section "Handler composition"
  handler_pattern="(aura_effects::.*Handler::new|PhysicalTimeHandler::new|RealRandomHandler::new|FilesystemStorageHandler::new|EncryptedStorageHandler::new|TcpNetworkHandler::new|RealCryptoHandler::new)"
  instantiation=$(rg --no-heading "$handler_pattern" crates/aura-protocol/src crates/aura-authenticate/src crates/aura-chat/src crates/aura-invitation/src crates/aura-recovery/src crates/aura-relational/src crates/aura-rendezvous/src crates/aura-sync/src -g "*.rs" -g "!tests/**/*" || true)
  if [ -n "$instantiation" ]; then
    warning "Direct aura-effects handler instantiation found (prefer EffectRegistry / composition):"
    echo "$instantiation"
  else
    info "No direct aura-effects handler instantiation in orchestration/feature crates"
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_TODOS" = true ]; then
  section "TODO/FIXME"
  todo_hits=$(rg --no-heading "TODO|FIXME" crates || true)
  if [ -n "$todo_hits" ]; then
    count=$(echo "$todo_hits" | wc -l | tr -d ' ')
    warning "TODO/FIXME markers present [$count]; full list numbered:" 
    nl -ba <<< "$todo_hits"
  fi
fi

echo -e "\n${BOLD}${CYAN}Summary${NC}"
if [ $VIOLATIONS -eq 0 ]; then
  echo -e "${GREEN}✔ No violations${NC}"
else
  echo -e "${RED}✖ $VIOLATIONS violation(s)${NC}"
fi
[ $WARNINGS -gt 0 ] && echo -e "${YELLOW}• $WARNINGS warning(s)${NC}"

exit $([ $VIOLATIONS -eq 0 ] && echo 0 || echo 1)
