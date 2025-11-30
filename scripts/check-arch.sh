#!/usr/bin/env bash
# Aura Architectural Compliance Checker (trimmed and opinionated)
set -euo pipefail

# Styling
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

usage() {
  cat <<'EOF'
Aura Architectural Compliance Checker

Usage: scripts/check-arch.sh [OPTIONS]

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

VIOLATIONS=0
VIOLATION_DETAILS=()

violation() { VIOLATIONS=$((VIOLATIONS+1)); VIOLATION_DETAILS+=("$1"); echo -e "${RED}✖${NC} $1"; }
# Warnings are treated as violations to enforce strict compliance
warning() { violation "$1"; }
info() { echo -e "${BLUE}•${NC} $1"; }

# Sort hits by layer (L1→L8) based on crate path.
sort_hits_by_layer() {
  while IFS= read -r entry; do
    [ -z "$entry" ] && continue
    path=${entry%%:*}
    crate=$(echo "$path" | cut -d/ -f2)
    layer=$(layer_of "$crate")
    [ "$layer" = "0" ] && layer=99
    printf "%02d:%s\n" "$layer" "$entry"
  done | sort -t: -k1,1n -k2,2 | sed 's/^[0-9][0-9]://'
}

# Helper to emit numbered violations with consistent formatting and layer ordering.
emit_hits() {
  local label="$1"; shift
  local hits="$1"
  if [ -n "$hits" ]; then
    local sorted
    sorted=$(printf "%s\n" "$hits" | sort_hits_by_layer)
    local idx=1
    while IFS= read -r entry; do
      [ -z "$entry" ] && continue
      violation "${label} [${idx}]: ${entry}"
      idx=$((idx+1))
    done <<< "$sorted"
  else
    info "${label}: none"
  fi
}

section() { echo -e "\n${BOLD}${CYAN}$1${NC}"; }

check_cargo() {
  if command -v cargo >/dev/null 2>&1; then
    return 0
  fi
  # Fallback to user toolchain (common in dev shells where PATH is trimmed)
  if [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
    return 0
  fi
  return 1
}

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
  section "Layer purity — keep aura-core interface-only; move impls to aura-effects (L3) or domain crates (L2); see docs/999_project_structure.md §Layer 1 and docs/001_system_architecture.md §6"
  # aura-core should only define traits/types (no impl of Effects)
  # Exclude: trait definitions, blanket impls (impl<...), and doc comments
  # Blanket impls include: extension traits and Arc<T> wrappers (both allowed exceptions per docs/999)
  if grep -R "impl.*Effects" crates/aura-core/src 2>/dev/null | grep -v "trait" | grep -v "impl<" | grep -v ":///" >/dev/null; then
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
  section "Dependency direction — remove upward deps (Lx→Ly where y>x); follow docs/999_project_structure.md dependency graph"
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
    violation "cargo unavailable; dependency direction not checked"
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_EFFECTS" = true ]; then
  section "Effects — infra traits only in aura-core; infra impls in aura-effects; app effects in domain crates; mocks in aura-testkit (docs/106_effect_system_and_runtime.md §1, docs/999_project_structure.md §Effect Trait Classification)"
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
  emit_hits "Infrastructure effects implemented outside aura-effects" "$infra_impls"

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
  emit_hits "Direct OS operations in domain crates (should use effect injection)" "$os_violations"

  section "Runtime coupling — keep foundation/spec crates runtime-agnostic; wrap tokio/async-std behind effects (docs/106_effect_system_and_runtime.md §3.5, docs/001_system_architecture.md §3)"
  runtime_pattern="tokio::|async_std::"
  runtime_hits=$(rg --no-heading "$runtime_pattern" crates -g "*.rs" || true)
  filtered_runtime=$(echo "$runtime_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-agent/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-cli/" \
    | grep -v "crates/aura-composition/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "#\\[tokio::test\\]" \
    | grep -v "#\\[async_std::test\\]" \
    | grep -v "#\\[tokio::main\\]" \
    | grep -v "/tests/" \
    | grep -v "/examples/" \
    | grep -v "test_macros.rs" \
    | grep -v "benches/" || true)
  emit_hits "Concrete runtime usage detected outside handler/composition layers (replace tokio/async-std with effect-injected abstractions)" "$filtered_runtime"

  section "Impure functions — route time/random/fs through effect traits; production handlers in aura-effects or runtime assembly (docs/106_effect_system_and_runtime.md §1.3, .claude/skills/common_patterns.md)"
  # Strict flag for direct wall-clock/random usage outside allowed areas
  impure_pattern="SystemTime::now|Instant::now|thread_rng\\(|rand::thread_rng|chrono::Utc::now|chrono::Local::now|rand::rngs::OsRng|rand::random"
  impure_hits=$(rg --no-heading "$impure_pattern" crates -g "*.rs" || true)
  # Allowlist: effect implementations, testkit mocks, simulator handlers, runtime assembly, CLI UI measurements
  # CLI code is allowed to use direct system time for UI measurements/metrics that don't affect protocol behavior
  # Filter out allowed areas and inline test modules
  # Note: Lines ending with .unwrap() or containing #[tokio::test] are likely test code
  filtered_impure=$(echo "$impure_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-agent/src/runtime/" \
    | grep -v "crates/aura-cli/" \
    | grep -v "tests/performance_regression.rs" \
    | grep -v "///" \
    | grep -v "//!" \
    | grep -v "//" \
    | grep -v "\.unwrap()" \
    | grep -v "#\[tokio::test\]" \
    | grep -v "#\[test\]" || true)
  # Second pass: filter out lines from files with inline #[cfg(test)] modules
  if [ -n "$filtered_impure" ]; then
    filtered_final=""
    while IFS= read -r line; do
      [ -z "$line" ] && continue
      file_path="${line%%:*}"
      # Skip if file contains #[cfg(test)] and this is a test module (heuristic)
      if [ -f "$file_path" ] && grep -q "#\[cfg(test)\]" "$file_path" 2>/dev/null; then
        # Get line number from match and check if it's after #[cfg(test)]
        match_line_text="${line#*:}"
        match_line_num=$(grep -n "$match_line_text" "$file_path" 2>/dev/null | head -1 | cut -d: -f1)
        cfg_test_line=$(grep -n "#\[cfg(test)\]" "$file_path" 2>/dev/null | head -1 | cut -d: -f1)
        if [ -n "$match_line_num" ] && [ -n "$cfg_test_line" ] && [ "$match_line_num" -gt "$cfg_test_line" ]; then
          continue  # Skip - this is in a test module
        fi
      fi
      filtered_final="${filtered_final}${line}"$'\n'
    done <<< "$filtered_impure"
    filtered_impure="$filtered_final"
  fi
  emit_hits "Impure functions detected outside effect implementations/testkit/runtime assembly" "$filtered_impure"

  section "Physical time guardrails — use PhysicalTimeEffects::sleep_ms; keep sleeps simulator-controllable (docs/106_effect_system_and_runtime.md §1.1, .claude/skills/common_patterns.md)"
  # Direct tokio::time::sleep instances should go through PhysicalTimeEffects
  tokio_sleep_hits=$(rg --no-heading "tokio::time::sleep" crates -g "*.rs" || true)
  filtered_tokio_sleep=$(echo "$tokio_sleep_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "/tests/" \
    | grep -v "/examples/" \
    | grep -v "benches/" || true)
  emit_hits "Direct tokio::time::sleep usage (should use PhysicalTimeEffects::sleep_ms)" "$filtered_tokio_sleep"

  # Check for direct sleeps from std/async-std (should use effect-injected time)
  sleep_pattern="std::thread::sleep|async_std::task::sleep"
  sleep_hits=$(rg --no-heading "$sleep_pattern" crates -g "*.rs" || true)
  filtered_sleep=$(echo "$sleep_hits" \
    | grep -v "crates/aura-effects/src/time.rs" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "/tests/" \
    | grep -v "benches/" || true)
  emit_hits "Direct sleeps detected (should be effect-injected/simulator-controlled)" "$filtered_sleep"

  section "Simulation control surfaces — inject randomness/IO/spawn via effects so simulator can control (docs/806_simulation_guide.md, .claude/skills/common_patterns.md)"
  sim_patterns="rand::random|rand::thread_rng|rand::rngs::OsRng|RngCore::fill_bytes|std::io::stdin|read_line\\(|std::thread::spawn"
  sim_hits=$(rg --no-heading "$sim_patterns" crates -g "*.rs" || true)
  filtered_sim=$(echo "$sim_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-agent/src/runtime/" \
    | grep -v "/tests/" \
    | grep -v "///" \
    | grep -v "//!" \
    | grep -v "//" || true)
  emit_hits "Potential non-injected randomness/IO/spawn (should be simulator-controllable; see docs/806_simulation_guide.md and .claude/skills/common_patterns.md)" "$filtered_sim"

  section "Pure interpreter alignment — migrate to GuardSnapshot + pure guard eval + EffectCommand interpreter (docs/106_effect_system_and_runtime.md §8, docs/001_system_architecture.md §2.1-2.3)"
  guard_bridge_hits=$(
    rg --no-heading "GuardEffectSystem" crates -g "*.rs" || true
  )
  guard_block_on_hits=$(
    rg --no-heading "futures::executor::block_on" crates -g "*.rs" || true
  )
  sync_output=$(printf "%s\n%s" "$guard_bridge_hits" "$guard_block_on_hits" | sed '/^$/d' | sort -u)
  emit_hits "Synchronous guard/effect bridges detected (migrate to pure snapshot + EffectCommand + interpreter; see docs/106_effect_system_and_runtime.md and docs/806_simulation_guide.md)" "$sync_output"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_GUARDS" = true ]; then
  section "Guard chain — all TransportEffects sends must flow through CapGuard → FlowGuard → JournalCoupler (docs/108_transport_and_information_flow.md, docs/001_system_architecture.md §2.1)"
  transport_sends=$(rg --no-heading "TransportEffects::(send|open_channel)" crates -g "*.rs" || true)
  guard_allowlist="crates/aura-protocol/src/guards|crates/aura-protocol/src/handlers/sessions|tests/|crates/aura-testkit/"
  bypass_hits=$(echo "$transport_sends" | grep -Ev "$guard_allowlist" || true)
  emit_hits "Potential guard-chain bypass (TransportEffects send/open outside guard modules)" "$bypass_hits"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_INVARIANTS" = true ]; then
  section "Invariant docs — INVARIANTS.md must include required headings; model after docs/005_system_invariants.md"
  invariant_files=$(find crates -name INVARIANTS.md 2>/dev/null | sort)
  if [ -z "$invariant_files" ]; then
    violation "Invariant docs: none found"
  else
    for inv in $invariant_files; do
      missing=()
      for heading in "Invariant Name" "Enforcement Locus" "Failure Mode" "Detection Method"; do
        if ! grep -q "$heading" "$inv"; then
          missing+=("$heading")
        fi
      done
      if [ ${#missing[@]} -gt 0 ]; then
        violation "Invariant doc missing sections [$(IFS=,; echo "${missing[*]}")]: $inv"
      else
        info "Invariant doc OK: $inv"
      fi
    done
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_REG" = true ]; then
  section "Handler composition — instantiate aura-effects via EffectRegistry/aura-composition, not direct new(); see docs/106_effect_system_and_runtime.md §3.3 and docs/999_project_structure.md §Layer 3"
  handler_pattern="(aura_effects::.*Handler::new|PhysicalTimeHandler::new|RealRandomHandler::new|FilesystemStorageHandler::new|EncryptedStorageHandler::new|TcpNetworkHandler::new|RealCryptoHandler::new)"
  instantiation=$(rg --no-heading "$handler_pattern" crates/aura-protocol/src crates/aura-authenticate/src crates/aura-chat/src crates/aura-invitation/src crates/aura-recovery/src crates/aura-relational/src crates/aura-rendezvous/src crates/aura-sync/src -g "*.rs" -g "!tests/**/*" || true)
  emit_hits "Direct aura-effects handler instantiation found (prefer EffectRegistry / composition)" "$instantiation"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_TODOS" = true ]; then
  section "Production placeholders — replace nil UUIDs/placeholder text with real IDs/derivations (see docs/105_identifiers_and_boundaries.md, docs/001_system_architecture.md §1.4)"
  placeholder_hits=$(rg --no-heading -i "uuid::nil\\(\\)|uuid::nil\\(|uuid::nil\\)|placeholder implementation|placeholder|for now" crates -g "*.rs" \
    | grep -v "/tests/" \
    | grep -v "/benches/" \
    | grep -v "/examples/" || true)
  if [ -n "$placeholder_hits" ]; then
    formatted=$(while IFS= read -r entry; do
      [ -z "$entry" ] && continue
      echo "$entry -- Action: derive real identifiers via AuthorityId/ContextId or deterministic key derivation"
    done <<< "$placeholder_hits")
    emit_hits "Placeholder identity/ID use" "$formatted"
  else
    info "Placeholder identity/ID use: none"
  fi

  section "Deterministic algorithm TODOs — replace vague notes with implemented deterministic paths (docs/108_transport_and_information_flow.md, docs/003_information_flow_contract.md)"
  deterministic_hits=$(rg --no-heading -i "deterministic algorithm" crates -g "*.rs" \
    | grep -v "/tests/" \
    | grep -v "/benches/" \
    | grep -v "/examples/" || true)
  if [ -n "$deterministic_hits" ]; then
    formatted=$(while IFS= read -r entry; do
      [ -z "$entry" ] && continue
      echo "$entry -- Action: implement deterministic selection/ordering per transport/guard specs; avoid entropy leaks"
    done <<< "$deterministic_hits")
    emit_hits "Deterministic algorithm stub" "$formatted"
  else
    info "Deterministic algorithm stubs: none"
  fi

  section "Temporary context fallbacks — ensure real context resolution instead of temp contexts (docs/103_relational_contexts.md, docs/001_system_architecture.md §1.4)"
  temp_ctx_hits=$(rg --no-heading -i "temporary context|temp context" crates -g "*.rs" \
    | grep -v "/tests/" \
    | grep -v "/benches/" \
    | grep -v "/examples/" || true)
  if [ -n "$temp_ctx_hits" ]; then
    formatted=$(while IFS= read -r entry; do
      [ -z "$entry" ] && continue
      echo "$entry -- Action: resolve ContextId via relational/journal state; remove temp context creation to avoid guard bypass"
    done <<< "$temp_ctx_hits")
    emit_hits "Temporary context fallback" "$formatted"
  else
    info "Temporary context fallbacks: none"
  fi

  section "TODO/FIXME — convert to tracked issues or implement; prioritize architecture/compliance blockers first"
  todo_hits=$(rg --no-heading "TODO|FIXME" crates || true)
  if [ -n "$todo_hits" ]; then
    sorted_todos=$(while IFS= read -r line; do
      path=${line%%:*}
      crate=$(echo "$path" | cut -d/ -f2)
      layer=$(layer_of "$crate")
      [ "$layer" = "0" ] && layer=99
      printf "%02d:%s\n" "$layer" "$line"
    done <<< "$todo_hits" | sort -t: -k1,1n -k2,2 | sed 's/^[0-9]\\{2\\}://')
    emit_hits "TODO/FIXME" "$sorted_todos"
  else
    info "TODO/FIXME: none"
  fi

  section "Incomplete markers — replace \"in production\"/WIP text with TODOs or complete implementation per docs/805_development_patterns.md"
  incomplete_pattern="in production[^\\n]*(would|should|not)|stub|not implemented|unimplemented|temporary|workaround|hacky|\\bWIP\\b|\\bTBD\\b|prototype|future work|to be implemented"
  incomplete_hits=$(rg --no-heading -i "$incomplete_pattern" crates -g "*.rs" || true)
  filtered_incomplete=$(echo "$incomplete_hits" \
    | grep -v "/tests/" \
    | grep -v "/benches/" \
    | grep -v "/examples/" \
    | grep -E "//" || true)
  if [ -n "$filtered_incomplete" ]; then
    emit_hits "Incomplete/WIP marker" "$filtered_incomplete"
  else
    info "Incomplete/WIP markers: none"
  fi
fi

echo -e "\n${BOLD}${CYAN}Summary${NC}"
if [ $VIOLATIONS -eq 0 ]; then
  echo -e "${GREEN}✔ No violations${NC}"
else
  echo -e "${RED}✖ $VIOLATIONS violation(s)${NC}"
fi

exit $([ $VIOLATIONS -eq 0 ] && echo 0 || echo 1)
