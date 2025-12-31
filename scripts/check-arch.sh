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
  --crypto         Crypto library usage boundaries (ed25519_dalek, OsRng, getrandom)
  --concurrency    Concurrency hygiene (block_in_place, unbounded channels)
  --reactive       TUI reactive data model (signals as source of truth, no domain data in props)
  --ui             UI boundary checks (aura-terminal uses aura_app::ui facade only)
  --workflows      aura-app workflow hygiene (runtime access, parsing helpers, signal access)
  --serialization  Serialization format enforcement (DAG-CBOR canonical, no bincode)
  --style          Rust style guide rules (usize in wire formats, bounded collections, etc.)
  --layer N[,M...] Filter output to specific layer numbers (1-8); repeatable
  --quick          Run fast checks only (skip todos, placeholders)
  -v, --verbose    Show more detail (allowlisted paths, etc.)
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
RUN_CRYPTO=false
RUN_CONCURRENCY=false
RUN_REACTIVE=false
RUN_UI=false
RUN_WORKFLOWS=false
RUN_SERIALIZATION=false
RUN_STYLE=false
RUN_QUICK=false
VERBOSE=false
LAYER_FILTERS=()

while [[ $# -gt 0 ]]; do
  case $1 in
    --layers) RUN_ALL=false; RUN_LAYERS=true ;;
    --deps) RUN_ALL=false; RUN_DEPS=true ;;
    --effects) RUN_ALL=false; RUN_EFFECTS=true ;;
    --guards) RUN_ALL=false; RUN_GUARDS=true ;;
    --invariants) RUN_ALL=false; RUN_INVARIANTS=true ;;
    --todos) RUN_ALL=false; RUN_TODOS=true ;;
    --registration) RUN_ALL=false; RUN_REG=true ;;
    --crypto) RUN_ALL=false; RUN_CRYPTO=true ;;
    --concurrency) RUN_ALL=false; RUN_CONCURRENCY=true ;;
    --reactive) RUN_ALL=false; RUN_REACTIVE=true ;;
    --ui) RUN_ALL=false; RUN_UI=true ;;
    --workflows) RUN_ALL=false; RUN_WORKFLOWS=true ;;
    --serialization) RUN_ALL=false; RUN_SERIALIZATION=true ;;
    --style) RUN_ALL=false; RUN_STYLE=true ;;
    --layer)
      if [[ -z "${2-}" ]]; then
        echo "--layer requires a layer number (1-8)"; exit 1
      fi
      IFS=',' read -r -a layers <<< "$2"
      for l in "${layers[@]}"; do
        LAYER_FILTERS+=("$l")
      done
      shift
      ;;
    --quick) RUN_QUICK=true ;;
    -v|--verbose) VERBOSE=true ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1"; usage; exit 1 ;;
  esac
  shift
done

# Quick mode skips slower/noisier checks
if [ "$RUN_QUICK" = true ] && [ "$RUN_ALL" = true ]; then
  RUN_LAYERS=true
  RUN_DEPS=true
  RUN_EFFECTS=true
  RUN_GUARDS=true
  RUN_INVARIANTS=true
  RUN_REG=true
  RUN_CRYPTO=true
  RUN_CONCURRENCY=true
  RUN_REACTIVE=true
  RUN_SERIALIZATION=true
  RUN_STYLE=true
  RUN_WORKFLOWS=true
  RUN_TODOS=false  # Skip todos in quick mode
  RUN_ALL=false
fi

VIOLATIONS=0
VIOLATION_DETAILS=()

violation() { VIOLATIONS=$((VIOLATIONS+1)); VIOLATION_DETAILS+=("$1"); echo -e "${RED}✖${NC} $1"; }
# Warnings are treated as violations to enforce strict compliance
warning() { violation "$1"; }
info() { echo -e "${BLUE}•${NC} $1"; }

# Extract layer from a file path
get_layer_from_path() {
  local path="$1"
  local crate
  crate=$(echo "$path" | sed 's|^crates/||' | cut -d/ -f1)
  layer_of "$crate"
}

# Sort hits by layer (L1→L8) based on crate path, preserving layer info.
sort_hits_by_layer() {
  while IFS= read -r entry; do
    [ -z "$entry" ] && continue
    path=${entry%%:*}
    crate=$(echo "$path" | cut -d/ -f2)
    layer=$(layer_of "$crate")
    [ "$layer" = "0" ] && layer=99
    printf "%02d:%s\n" "$layer" "$entry"
  done | sort -t: -k1,1n -k2,2
}

layer_filter_matches() {
  local layer="$1"
  # No filter -> always matches
  if [ ${#LAYER_FILTERS[@]} -eq 0 ]; then
    return 0
  fi
  for lf in "${LAYER_FILTERS[@]}"; do
    if [ "$layer" = "$lf" ]; then
      return 0
    fi
  done
  return 1
}

# Helper to emit numbered violations with consistent formatting and layer ordering.
# Output format: [Ln] label [idx]: path:content
emit_hits() {
  local label="$1"; shift
  local hits="$1"
  if [ -n "$hits" ]; then
    local sorted
    sorted=$(printf "%s\n" "$hits" | sort_hits_by_layer)
    local idx=1
    local any=false
    while IFS= read -r entry; do
      [ -z "$entry" ] && continue
      # Extract layer number (first 2 chars) and actual content
      local layer_num="${entry:0:2}"
      local content="${entry:3}"  # Skip "NN:"
      # Convert layer 99 back to "?" for unknown
      [ "$layer_num" = "99" ] && layer_num="?"
      # Remove leading zero
      layer_num="${layer_num#0}"
        # Apply layer filter if present
      if ! layer_filter_matches "$layer_num"; then
        continue
      fi
      any=true
      violation "[L${layer_num}] ${label} [${idx}]: ${content}"
      idx=$((idx+1))
    done <<< "$sorted"
    if [ "$any" = false ]; then
      info "${label}: none (filtered)"
    fi
  else
    info "${label}: none"
  fi
}

section() { echo -e "\n${BOLD}${CYAN}$1${NC}"; }
verbose() { [ "$VERBOSE" = true ] && echo -e "${BLUE}  ↳${NC} $1" || true; }

# Precise allowlists for impure operations
# These specify exact modules/files that legitimately need impure operations

# Infrastructure effect implementations (Layer 3)
EFFECT_HANDLER_ALLOWLIST="crates/aura-effects/src/"

# Test infrastructure (Layer 8) - mocks and test harnesses
# Also includes /testing/ directories which are L8-style test infrastructure in other layers
TEST_ALLOWLIST="crates/aura-testkit/|/tests/|/testing/|/examples/|benches/"

# Simulator (Layer 6/8) - simulation-specific impurity and test infrastructure (handlers, quint ITF loading)
SIMULATOR_ALLOWLIST="crates/aura-simulator/src/"

# Runtime assembly (Layer 6) - where effects are composed with real impls
# Includes runtime/ subdirectory, runtime_bridge_impl.rs, and builder/ (bootstrapping before effects exist)
RUNTIME_ALLOWLIST="crates/aura-agent/src/runtime/|crates/aura-agent/src/runtime_bridge_impl.rs|crates/aura-agent/src/builder/"

# App core (Layer 5) - cfg-gated for native builds only (#[cfg(not(target_arch = "wasm32"))])
# signal_sync.rs uses tokio::spawn for background forwarding tasks (native platform feature)
APP_NATIVE_ALLOWLIST="crates/aura-app/src/core/app.rs|crates/aura-app/src/core/signal_sync.rs"

# CLI entry points (Layer 7) - main.rs and bootstrap handlers where production starts
CLI_ENTRY_ALLOWLIST="crates/aura-terminal/src/main.rs"
# TUI bootstrap handler - needs fs access before effect system exists
TUI_BOOTSTRAP_ALLOWLIST="crates/aura-terminal/src/handlers/tui.rs"
# TUI infrastructure - low-level terminal plumbing (fd redirection, stdio capture)
TUI_INFRA_ALLOWLIST="crates/aura-terminal/src/tui/fullscreen_stdio.rs"

# Common filter for effect/impure checks
# Usage: filter_common_allowlist "$input" ["extra_pattern"]
filter_common_allowlist() {
  local input="$1"
  local extra="${2:-}"
  local result
  # Use -E for extended regex (alternation with |)
  # Filter doc comments (///) as they're examples, not actual code
  result=$(echo "$input" \
    | grep -v "$EFFECT_HANDLER_ALLOWLIST" \
    | grep -v "$SIMULATOR_ALLOWLIST" \
    | grep -Ev "$TEST_ALLOWLIST" \
    | grep -v "///" || true)
  if [ -n "$extra" ]; then
    result=$(echo "$result" | grep -Ev "$extra" || true)
  fi
  echo "$result"
}

# Counts for summary
# NOTE: Keep this script compatible with macOS bash 3.x (no associative arrays).

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
    aura-journal|aura-authorization|aura-signature|aura-store|aura-transport|aura-mpst|aura-macros) echo 2 ;;
    aura-effects|aura-composition) echo 3 ;;
    aura-protocol|aura-guards|aura-consensus|aura-amp|aura-anti-entropy) echo 4 ;;
    aura-authentication|aura-chat|aura-invitation|aura-recovery|aura-relational|aura-rendezvous|aura-sync|aura-app) echo 5 ;;
    aura-agent|aura-simulator) echo 6 ;;
    aura-terminal) echo 7 ;;
    aura-testkit|aura-quint) echo 8 ;;
    *) echo 0 ;;
  esac
}

if [ "$RUN_ALL" = true ] || [ "$RUN_LAYERS" = true ]; then
  section "Layer purity — keep aura-core interface-only; move impls to aura-effects (L3) or domain crates (L2); see docs/999_project_structure.md §Layer 1 and docs/001_system_architecture.md §6"
  # aura-core should only define traits/types (no impl of Effects)
  # Exclude: trait definitions, blanket impls (impl<...), and doc comments
  # Blanket impls include: extension traits and Arc<T> wrappers (both allowed exceptions per docs/999)
  # Use word boundary \bimpl\b to avoid false positives like "SimpleIntentEffects"
  if grep -RE "\bimpl\b.*Effects" crates/aura-core/src 2>/dev/null | grep -v "trait" | grep -v "impl<" | grep -v ":///" >/dev/null; then
    violation "aura-core contains effect implementations (should be interface-only)"
  else
    info "aura-core: interface-only (no effect impls)"
  fi

  # Domain crates should not depend on runtime/UI layers
  for crate in aura-authentication aura-app aura-chat aura-invitation aura-recovery aura-relational aura-rendezvous aura-sync; do
    if [ -d "crates/$crate" ]; then
      if grep -A20 "^\[dependencies\]" crates/$crate/Cargo.toml | grep -E "aura-agent|aura-simulator|aura-terminal" >/dev/null; then
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

  # Layer 4 dependency firewall: prevent upward deps into Layer 6+
  section "Layer 4 firewall — disallow dependencies on runtime/UI/testkit layers"
  l4_crates=(aura-protocol aura-guards aura-consensus aura-amp aura-anti-entropy)
  l4_blocked="aura-agent|aura-simulator|aura-app|aura-terminal|aura-testkit"
  for crate in "${l4_crates[@]}"; do
    if [ -f "crates/$crate/Cargo.toml" ]; then
      if rg -n "^(.*\\[dependencies\\].*|.*\\[dev-dependencies\\].*|.*\\[build-dependencies\\].*|.*$l4_blocked.*)" "crates/$crate/Cargo.toml" | rg -n "$l4_blocked" >/dev/null; then
        violation "$crate depends on Layer 6+ ($l4_blocked) — forbidden by firewall"
      else
        info "$crate: firewall clean"
      fi
    fi
  done
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_EFFECTS" = true ]; then
  section "Effects — infra traits only in aura-core; infra impls in aura-effects; app effects in domain crates; mocks in aura-testkit (docs/106_effect_system_and_runtime.md §1, docs/999_project_structure.md §Effect Trait Classification)"
  # Infrastructure effect traits must live in aura-core
  infra_traits="CryptoEffects|NetworkEffects|StorageEffects|PhysicalTimeEffects|LogicalClockEffects|OrderClockEffects|TimeAttestationEffects|RandomEffects|ConsoleEffects|ConfigurationEffects|LeakageEffects"
  infra_defs=$(find crates/ -name "*.rs" -not -path "*/aura-core/*" -exec grep -El "pub trait ($infra_traits)" {} + 2>/dev/null || true)
  if [ -n "$infra_defs" ]; then
    violation "Infrastructure effect traits defined outside aura-core:"
    echo "$infra_defs"
  else
    info "Infra effect traits defined only in aura-core"
  fi

  # aura-effects should stay stateless (except allowed infra caches: reactive signal registry, query fact cache)
  stateful_matches=$(grep -R "Arc<Mutex\|Arc<RwLock\|Rc<RefCell" crates/aura-effects/src 2>/dev/null | grep -v "test" | grep -v "reactive/handler.rs" | grep -v "query/handler.rs" || true)
  if [ -n "$stateful_matches" ]; then
    violation "aura-effects contains stateful constructs (should be stateless handlers)"
    echo "$stateful_matches"
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
  # Note: LeakageEffects is infrastructure (moved to infra_traits above)
  app_effects="JournalEffects|AuthorityEffects|FlowBudgetEffects|AuthorizationEffects|RelationalContextEffects|GuardianEffects|ChoreographicEffects|EffectApiEffects|SyncEffects"
  app_impls=$(grep -R "impl.*\($app_effects\)" crates/aura-effects/src 2>/dev/null | grep -v "test" || true)
  if [ -n "$app_impls" ]; then
    violation "Application effects implemented in aura-effects (should be in domain crates)"
  else
    info "No application effects implemented in aura-effects"
  fi

  # Check for direct OS operations in domain handlers
  domain_crates="aura-journal|aura-authorization|aura-signature|aura-store|aura-transport|aura-authentication|aura-recovery|aura-relational"
  os_violations=$(find crates/ -path "*/src/*" -name "*.rs" | grep -E "($domain_crates)" | xargs grep -l "std::fs::\|SystemTime::now\|thread_rng()" 2>/dev/null | grep -v "test" || true)
  emit_hits "Direct OS operations in domain crates (should use effect injection)" "$os_violations"

  # Check for direct std::fs usage outside handler layers (should use StorageEffects)
  # Allowed: effect handler impls (storage.rs), runtime assembly, tests, cfg-gated native code, TUI bootstrap
  fs_pattern="std::fs::|std::io::File|std::io::BufReader|std::io::BufWriter"
  fs_hits=$(rg --no-heading "$fs_pattern" crates -g "*.rs" || true)
  filtered_fs=$(filter_common_allowlist "$fs_hits" "$RUNTIME_ALLOWLIST|$APP_NATIVE_ALLOWLIST|$TUI_BOOTSTRAP_ALLOWLIST|$TUI_INFRA_ALLOWLIST")
  # Additional filter: skip lines in files after #[cfg(test)] (inline test modules)
  if [ -n "$filtered_fs" ]; then
    filtered_fs_final=""
    while IFS= read -r line; do
      [ -z "$line" ] && continue
      file_path="${line%%:*}"
      # Skip if file contains #[cfg(test)] and this line is in the test module
      if [ -f "$file_path" ] && grep -q "#\[cfg(test)\]" "$file_path" 2>/dev/null; then
        match_line_text="${line#*:}"
        match_line_num=$(grep -n "$match_line_text" "$file_path" 2>/dev/null | head -1 | cut -d: -f1)
        cfg_test_line=$(grep -n "#\[cfg(test)\]" "$file_path" 2>/dev/null | head -1 | cut -d: -f1)
        if [ -n "$match_line_num" ] && [ -n "$cfg_test_line" ] && [ "$match_line_num" -gt "$cfg_test_line" ]; then
          continue  # Skip - this is in a test module
        fi
      fi
      filtered_fs_final="${filtered_fs_final}${line}"$'\n'
    done <<< "$filtered_fs"
    filtered_fs="$filtered_fs_final"
  fi
  emit_hits "Direct std::fs usage (should use StorageEffects)" "$filtered_fs"
  verbose "Allowed: aura-effects/src/, aura-simulator/src/handlers/, aura-agent/src/runtime/, aura-app/src/core/app.rs (cfg-gated), aura-terminal/src/handlers/tui.rs (bootstrap), tests/, testing/, #[cfg(test)] modules"

  # Check for direct std::net usage outside handler layers (should use NetworkEffects)
  # Allowed: effect handler impls (network.rs), runtime assembly, tests
  net_pattern="std::net::|TcpStream|TcpListener|UdpSocket"
  net_hits=$(rg --no-heading "$net_pattern" crates -g "*.rs" || true)
  filtered_net=$(filter_common_allowlist "$net_hits" "$RUNTIME_ALLOWLIST")
  emit_hits "Direct std::net usage (should use NetworkEffects)" "$filtered_net"
  verbose "Allowed: aura-effects/src/, aura-simulator/src/handlers/, aura-agent/src/runtime/, tests/"

  section "Runtime coupling — keep foundation/spec crates runtime-agnostic; wrap tokio/async-std behind effects (docs/106_effect_system_and_runtime.md §3.5, docs/001_system_architecture.md §3)"
  runtime_pattern="tokio::|async_std::"
  runtime_hits=$(rg --no-heading -n "$runtime_pattern" crates -g "*.rs" || true)
  # Allowlist: effect handlers, agent runtime, simulator, terminal UI, composition, testkit, app core (native feature), tests
  # Layer 6 (runtime) and Layer 7 (UI) are allowed to use tokio directly
  # Layer 5 aura-app uses tokio for signal forwarding (cfg-gated for native platforms)
  # Note: aura-authorization/storage_authorization.rs uses tokio::sync::RwLock for AuthorizedStorageHandler
  # which is a handler wrapper that should eventually move to aura-composition (tracked technical debt)
  # Note: aura-core/effects/reactive.rs uses tokio::sync::broadcast for SignalStream<T> which is
  # part of the ReactiveEffects trait API. This should be abstracted to a runtime-agnostic stream
  # trait in the future (tracked technical debt: abstract SignalStreamReceiver trait)
  filtered_runtime=$(echo "$runtime_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-agent/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-terminal/" \
    | grep -v "crates/aura-composition/" \
    | grep -v "crates/aura-testkit/" \
    | grep -Ev "$APP_NATIVE_ALLOWLIST" \
    | grep -v "crates/aura-authorization/src/storage_authorization.rs" \
    | grep -v "crates/aura-core/src/effects/reactive.rs" \
    | grep -v "#\\[tokio::test\\]" \
    | grep -v "#\\[async_std::test\\]" \
    | grep -v "#\\[tokio::main\\]" \
    | grep -v "/tests/" \
    | grep -v "/examples/" \
    | grep -v "test_macros.rs" \
    | grep -v "benches/" || true)
  # Second pass: filter out lines from files with inline #[cfg(test)] modules
  if [ -n "$filtered_runtime" ]; then
    filtered_final=""
    while IFS= read -r line; do
      [ -z "$line" ] && continue
      file_path="${line%%:*}"
      # Skip if file contains #[cfg(test)] and this is a test module (heuristic)
      if [ -f "$file_path" ] && grep -q "#\[cfg(test)\]" "$file_path" 2>/dev/null; then
        # Extract line number (format: file:linenum:content)
        match_line_num=$(echo "$line" | cut -d: -f2)
        cfg_test_line=$(grep -n "#\[cfg(test)\]" "$file_path" 2>/dev/null | head -1 | cut -d: -f1)
        if [ -n "$match_line_num" ] && [ -n "$cfg_test_line" ] && [ "$match_line_num" -gt "$cfg_test_line" ]; then
          continue  # Skip - this is in a test module
        fi
      fi
      filtered_final="${filtered_final}${line}"$'\n'
    done <<< "$filtered_runtime"
    filtered_runtime="$filtered_final"
  fi
  emit_hits "Concrete runtime usage detected outside handler/composition layers (replace tokio/async-std with effect-injected abstractions)" "$filtered_runtime"

  section "Aura-app runtime-agnostic surface — no tokio/async-std in aura-app"
  app_runtime_hits=$(rg --no-heading -n "tokio::|async_std::" crates/aura-app/src -g "*.rs" || true)
  filtered_app_runtime=$(echo "$app_runtime_hits" \
    | grep -v "#\\[tokio::test\\]" \
    | grep -v "#\\[async_std::test\\]" \
    | grep -v "/tests/" \
    | grep -v "/benches/" || true)
  emit_hits "tokio/async-std usage in aura-app (should be runtime-agnostic)" "$filtered_app_runtime"

  section "Impure functions — route time/random/fs through effect traits; production handlers in aura-effects or runtime assembly (docs/106_effect_system_and_runtime.md §1.3, .claude/skills/patterns/SKILL.md)"
  # Strict flag for direct wall-clock/random usage outside allowed areas
  impure_pattern="SystemTime::now|Instant::now|thread_rng\\(|rand::thread_rng|chrono::Utc::now|chrono::Local::now|rand::rngs::OsRng|rand::random"
  impure_hits=$(rg --no-heading "$impure_pattern" crates -g "*.rs" || true)
  # Allowlist: effect handlers, testkit, simulator, agent runtime, terminal UI, tests, benches
  # Terminal UI is allowed to use direct system time for UI measurements/metrics that don't affect protocol behavior
  # Note: Lines ending with .unwrap() or containing #[tokio::test] are likely test code
  filtered_impure=$(echo "$impure_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -Ev "$RUNTIME_ALLOWLIST" \
    | grep -v "crates/aura-terminal/" \
    | grep -v "/tests/" \
    | grep -v "/benches/" \
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

  section "Physical time guardrails — use PhysicalTimeEffects::sleep_ms; keep sleeps simulator-controllable (docs/106_effect_system_and_runtime.md §1.1, .claude/skills/patterns/SKILL.md)"
  # Direct tokio::time::sleep instances should go through PhysicalTimeEffects
  # Use -n for line numbers so we can filter by test module position
  tokio_sleep_hits=$(rg --no-heading -n "tokio::time::sleep" crates -g "*.rs" || true)
  # Allowlist: effect handlers (time.rs), simulator, testkit, tests, aura-terminal (L7 UI)
  # aura-agent should use PhysicalTimeEffects::sleep_ms for simulator determinism
  # Also filter out inline #[cfg(test)] module content
  filtered_tokio_sleep=""
  if [[ -n "$tokio_sleep_hits" ]]; then
    # First pass: basic path filtering
    path_filtered=$(echo "$tokio_sleep_hits" \
      | grep -v "crates/aura-effects/" \
      | grep -v "crates/aura-simulator/" \
      | grep -v "crates/aura-testkit/" \
      | grep -v "crates/aura-terminal/" \
      | grep -v "/tests/" \
      | grep -v "/examples/" \
      | grep -v "benches/" || true)
    # Second pass: filter out matches that are in inline test modules
    # Format is file:linenum:content - extract linenum and check against #[cfg(test)] position
    if [[ -n "$path_filtered" ]]; then
      while IFS= read -r hit; do
        [[ -z "$hit" ]] && continue
        file=$(echo "$hit" | cut -d: -f1)
        linenum=$(echo "$hit" | cut -d: -f2)
        # Check if this line is within a #[cfg(test)] module (after the marker)
        test_mod_line=$(grep -n '#\[cfg(test)\]' "$file" 2>/dev/null | head -1 | cut -d: -f1 || echo "99999")
        if [[ "$linenum" =~ ^[0-9]+$ ]] && [[ "$linenum" -lt "$test_mod_line" ]]; then
          # Line is before test module, include it (strip line number for display)
          filtered_tokio_sleep+="${file}:$(echo "$hit" | cut -d: -f3-)"$'\n'
        elif ! [[ "$linenum" =~ ^[0-9]+$ ]]; then
          # No line number format, include as-is
          filtered_tokio_sleep+="$hit"$'\n'
        fi
        # Lines after test_mod_line are in test modules, skip them
      done <<< "$path_filtered"
      filtered_tokio_sleep="${filtered_tokio_sleep%$'\n'}"
    fi
  fi
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

  section "Sync protocol runtime neutrality — no tokio/async-std in aura-sync protocols"
  sync_protocol_runtime=$(rg --no-heading -n "tokio::|async_std::" crates/aura-sync/src/protocols -g "*.rs" || true)
  filtered_sync_protocol_runtime=$(echo "$sync_protocol_runtime" \
    | grep -v "///" \
    | grep -v "//!" \
    | grep -v "//" || true)
  if [ -n "$filtered_sync_protocol_runtime" ]; then
    emit_hits "Runtime-specific usage in aura-sync protocols (replace with effect-injected abstractions)" "$filtered_sync_protocol_runtime"
  else
    info "aura-sync protocols: no runtime-specific usage"
  fi

  section "Simulation control surfaces — inject randomness/IO/spawn via effects so simulator can control (docs/806_simulation_guide.md, .claude/skills/patterns/SKILL.md)"
  sim_patterns="rand::random|rand::thread_rng|rand::rngs::OsRng|RngCore::fill_bytes|std::io::stdin|read_line\\(|std::thread::spawn"
  sim_hits=$(rg --no-heading "$sim_patterns" crates -g "*.rs" || true)
  # TUI_BLOCKON_ALLOWLIST: TUI sync/async bridge helper using std::thread::spawn to avoid
  # "Cannot start a runtime from within a runtime" panic. Underlying storage ops go through
  # effect handlers (PathFilesystemStorageHandler). See handlers/tui.rs block_on().
  TUI_BLOCKON_ALLOWLIST="crates/aura-terminal/src/handlers/tui.rs"
  filtered_sim=$(echo "$sim_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-agent/src/runtime/" \
    | grep -v "$TUI_BLOCKON_ALLOWLIST" \
    | grep -v "/tests/" \
    | grep -v "///" \
    | grep -v "//!" \
    | grep -v "//" || true)
  emit_hits "Potential non-injected randomness/IO/spawn (should be simulator-controllable; see docs/806_simulation_guide.md and .claude/skills/patterns/SKILL.md)" "$filtered_sim"

  section "Pure interpreter alignment — migrate to GuardSnapshot + pure guard eval + EffectCommand interpreter (docs/106_effect_system_and_runtime.md §8, docs/001_system_architecture.md §2.1-2.3)"
  guard_bridge_hits=$(
    rg --no-heading "GuardEffectSystem" crates -g "*.rs" || true
  )
  guard_block_on_hits=$(
    rg --no-heading "futures::executor::block_on" crates -g "*.rs" || true
  )
  sync_output=$(printf "%s\n%s" "$guard_bridge_hits" "$guard_block_on_hits" | sed '/^$/d' | sort -u)
  emit_hits "Synchronous guard/effect bridges detected (migrate to pure snapshot + EffectCommand + interpreter; see docs/106_effect_system_and_runtime.md and docs/806_simulation_guide.md)" "$sync_output"

  section "Identifier determinism — avoid entropy-consuming ID creation; use deterministic constructors for tests"
  # Reference: .claude/skills/patterns/SKILL.md (Test Determinism Violations section)
  # Reference: docs/805_testing_guide.md (Deterministic Identifier Generation section)

  # Check for AuthorityId::new(), ContextId::new(), DeviceId::new() which use system entropy
  # Allowed only in: effect handlers (random.rs), runtime assembly, CLI entry point, tests
  entropy_id_pattern="AuthorityId::new\\(\\)|ContextId::new\\(\\)|DeviceId::new\\(\\)"
  entropy_id_hits=$(rg --no-heading "$entropy_id_pattern" crates -g "*.rs" || true)
  filtered_entropy_ids=$(echo "$entropy_id_hits" \
    | grep -v "$EFFECT_HANDLER_ALLOWLIST" \
    | grep -Ev "$RUNTIME_ALLOWLIST" \
    | grep -v "$CLI_ENTRY_ALLOWLIST" \
    | grep -Ev "$TEST_ALLOWLIST" || true)
  if [ -n "$filtered_entropy_ids" ]; then
    # Sort by layer and emit with layer prefix, respecting layer filters
    sorted_ids=$(printf "%s\n" "$filtered_entropy_ids" | sort_hits_by_layer)
    any=false
    while IFS= read -r entry; do
      [ -z "$entry" ] && continue
      layer_num="${entry:0:2}"
      content="${entry:3}"
      [ "$layer_num" = "99" ] && layer_num="?"
      layer_num="${layer_num#0}"
      if ! layer_filter_matches "$layer_num"; then
        continue
      fi
      any=true
      violation "[L${layer_num}] Entropy-consuming ID: $content"
      echo -e "    ${YELLOW}Fix:${NC} Use XxxId::new_from_entropy([n; 32]) or ContextId::from_uuid(Uuid::from_bytes([n; 16]))"
      echo -e "    ${YELLOW}Ref:${NC} .claude/skills/patterns/SKILL.md §Test Determinism Violations"
    done <<< "$sorted_ids"
    if [ "$any" = false ]; then
      info "Entropy-consuming identifiers: none (filtered)"
    fi
  else
    info "Entropy-consuming identifiers (AuthorityId::new, ContextId::new, DeviceId::new): none"
  fi

  # Check for Uuid::new_v4() which uses system entropy
  # Allowed only in: effect handlers, runtime assembly, CLI entry point, tests
  uuid_v4_pattern="Uuid::new_v4|uuid::Uuid::new_v4"
  uuid_v4_hits=$(rg --no-heading "$uuid_v4_pattern" crates -g "*.rs" || true)
  filtered_uuid_v4=$(echo "$uuid_v4_hits" \
    | grep -v "$EFFECT_HANDLER_ALLOWLIST" \
    | grep -Ev "$RUNTIME_ALLOWLIST" \
    | grep -v "$CLI_ENTRY_ALLOWLIST" \
    | grep -Ev "$TEST_ALLOWLIST" || true)
  if [ -n "$filtered_uuid_v4" ]; then
    # Sort by layer and emit with layer prefix, respecting layer filters
    sorted_uuids=$(printf "%s\n" "$filtered_uuid_v4" | sort_hits_by_layer)
    any=false
    while IFS= read -r entry; do
      [ -z "$entry" ] && continue
      layer_num="${entry:0:2}"
      content="${entry:3}"
      [ "$layer_num" = "99" ] && layer_num="?"
      layer_num="${layer_num#0}"
      if ! layer_filter_matches "$layer_num"; then
        continue
      fi
      any=true
      violation "[L${layer_num}] Entropy-consuming UUID: $content"
      echo -e "    ${YELLOW}Fix:${NC} Use Uuid::nil() for placeholders or Uuid::from_bytes([n; 16]) for deterministic unique IDs"
      echo -e "    ${YELLOW}Ref:${NC} .claude/skills/patterns/SKILL.md §Test Determinism Violations"
    done <<< "$sorted_uuids"
    if [ "$any" = false ]; then
      info "Entropy-consuming UUIDs: none (filtered)"
    fi
  else
    info "Entropy-consuming UUIDs (Uuid::new_v4): none"
  fi

  # Check for rand::random and thread_rng outside allowed areas
  rand_pattern="rand::random|thread_rng\\(\\)|rand::thread_rng"
  rand_hits=$(rg --no-heading "$rand_pattern" crates -g "*.rs" || true)
  filtered_rand=$(echo "$rand_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-agent/src/runtime/" \
    | grep -v "/tests/" \
    | grep -v "///" \
    | grep -v "//!" || true)
  if [ -n "$filtered_rand" ]; then
    # Sort by layer and emit with layer prefix, respecting layer filters
    sorted_rand=$(printf "%s\n" "$filtered_rand" | sort_hits_by_layer)
    any=false
    while IFS= read -r entry; do
      [ -z "$entry" ] && continue
      layer_num="${entry:0:2}"
      content="${entry:3}"
      [ "$layer_num" = "99" ] && layer_num="?"
      layer_num="${layer_num#0}"
      if ! layer_filter_matches "$layer_num"; then
        continue
      fi
      any=true
      violation "[L${layer_num}] Direct randomness: $content"
      echo -e "    ${YELLOW}Fix:${NC} Use RandomEffects trait for production code; use deterministic seeds/bytes for tests"
      echo -e "    ${YELLOW}Ref:${NC} .claude/skills/patterns/SKILL.md §Test Determinism Violations, docs/805_testing_guide.md"
    done <<< "$sorted_rand"
    if [ "$any" = false ]; then
      info "Direct randomness: none (filtered)"
    fi
  else
    info "Direct randomness (rand::random, thread_rng): none"
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_GUARDS" = true ]; then
  section "Guard chain — all TransportEffects sends must flow through CapGuard → FlowGuard → JournalCoupler (docs/108_transport_and_information_flow.md, docs/001_system_architecture.md §2.1)"
  transport_sends=$(rg --no-heading "TransportEffects::(send|open_channel)" crates -g "*.rs" || true)
  guard_allowlist="crates/aura-guards/src/guards|crates/aura-protocol/src/handlers/sessions|crates/aura-agent/src/runtime/effects.rs|tests/|crates/aura-testkit/"
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
  instantiation=$(rg --no-heading "$handler_pattern" crates/aura-protocol/src crates/aura-authentication/src crates/aura-chat/src crates/aura-invitation/src crates/aura-recovery/src crates/aura-relational/src crates/aura-rendezvous/src crates/aura-sync/src -g "*.rs" -g "!tests/**/*" || true)
  emit_hits "Direct aura-effects handler instantiation found (prefer EffectRegistry / composition)" "$instantiation"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_CRYPTO" = true ]; then
  section "Crypto library boundaries — route crypto through aura-core wrappers; keep ed25519_dalek/OsRng/getrandom in allowed locations (work/crypto.md, docs/106_effect_system_and_runtime.md)"

  # Allowed locations for direct crypto library usage:
  # - Layer 1: aura-core/src/crypto/* (wrapper implementations)
  # - Layer 1: aura-core/src/types/authority.rs (type aliases - known design issue)
  # - Layer 3: aura-effects/src/* (production handlers)
  # - Layer 8: aura-testkit/* (test infrastructure)
  # - Test modules: /tests/, *_test.rs
  CRYPTO_ALLOWLIST="crates/aura-core/src/crypto/|crates/aura-core/src/types/authority.rs|crates/aura-effects/src/|crates/aura-testkit/|/tests/|_test\\.rs"

  # Allowed locations for direct randomness (OsRng, getrandom):
  # - Layer 3: aura-effects/src/* (production handlers)
  # - Layer 8: aura-testkit/* (test infrastructure)
  # - Test modules: /tests/, *_test.rs
  # - #[cfg(test)] modules (detected by context)
  RANDOMNESS_ALLOWLIST="crates/aura-effects/src/|crates/aura-testkit/|/tests/|_test\\.rs"

  # Check for direct ed25519_dalek imports outside allowed locations
  ed25519_imports=$(rg --no-heading "use ed25519_dalek" crates -g "*.rs" || true)
  filtered_ed25519=$(echo "$ed25519_imports" | grep -Ev "$CRYPTO_ALLOWLIST" || true)
  if [ -n "$filtered_ed25519" ]; then
    emit_hits "Direct ed25519_dalek import (use aura_core::crypto::ed25519 wrappers instead)" "$filtered_ed25519"
    echo -e "    ${YELLOW}Allowed locations:${NC}"
    echo -e "      - crates/aura-core/src/crypto/* (L1 wrappers)"
    echo -e "      - crates/aura-effects/src/* (L3 handlers)"
    echo -e "      - crates/aura-testkit/* (L8 testing)"
    echo -e "      - /tests/ directories and *_test.rs files"
  else
    info "Direct ed25519_dalek imports: none outside allowed locations"
  fi

  # Check for direct OsRng usage outside allowed locations
  # Filter out comments and #[cfg(test)] code
  osrng_usage=$(rg --no-heading "OsRng" crates -g "*.rs" || true)
  filtered_osrng=$(echo "$osrng_usage" \
    | grep -v "///" \
    | grep -v "//!" \
    | grep -v "// " \
    | grep -Ev "$RANDOMNESS_ALLOWLIST" || true)
  # Additional filter: skip lines in files after #[cfg(test)]
  if [ -n "$filtered_osrng" ]; then
    osrng_final=""
    while IFS= read -r line; do
      [ -z "$line" ] && continue
      file_path="${line%%:*}"
      if [ -f "$file_path" ] && grep -q "#\[cfg(test)\]" "$file_path" 2>/dev/null; then
        # Get line content and check if it's in test module
        match_content="${line#*:}"
        match_line_num=$(grep -n "$match_content" "$file_path" 2>/dev/null | head -1 | cut -d: -f1)
        cfg_test_line=$(grep -n "#\[cfg(test)\]" "$file_path" 2>/dev/null | head -1 | cut -d: -f1)
        if [ -n "$match_line_num" ] && [ -n "$cfg_test_line" ] && [ "$match_line_num" -gt "$cfg_test_line" ]; then
          continue  # Skip - in test module
        fi
      fi
      osrng_final="${osrng_final}${line}"$'\n'
    done <<< "$filtered_osrng"
    filtered_osrng="$osrng_final"
  fi
  if [ -n "$filtered_osrng" ]; then
    emit_hits "Direct OsRng usage (use RandomEffects trait instead)" "$filtered_osrng"
    echo -e "    ${YELLOW}Allowed locations:${NC}"
    echo -e "      - crates/aura-effects/src/* (L3 handlers)"
    echo -e "      - crates/aura-testkit/* (L8 testing)"
    echo -e "      - #[cfg(test)] modules"
  else
    info "Direct OsRng usage: none outside allowed locations"
  fi

  # Check for direct getrandom usage outside allowed locations
  getrandom_usage=$(rg --no-heading "getrandom::" crates -g "*.rs" || true)
  filtered_getrandom=$(echo "$getrandom_usage" \
    | grep -v "///" \
    | grep -v "//" \
    | grep -Ev "$RANDOMNESS_ALLOWLIST" || true)
  if [ -n "$filtered_getrandom" ]; then
    emit_hits "Direct getrandom usage (use RandomEffects trait instead)" "$filtered_getrandom"
    echo -e "    ${YELLOW}Allowed locations:${NC}"
    echo -e "      - crates/aura-effects/src/* (L3 handlers)"
    echo -e "      - crates/aura-testkit/* (L8 testing)"
  else
    info "Direct getrandom usage: none outside allowed locations"
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_CONCURRENCY" = true ]; then
  section "Concurrency hygiene — avoid block_in_place/block_on and unbounded channels in production code"

  # Flag blocking bridges in async code.
  block_bridge_hits=$(rg --no-heading "tokio::task::block_in_place|Handle::current\\(\\)\\.block_on" crates -g "*.rs" || true)
  filtered_block_bridge=$(filter_common_allowlist "$block_bridge_hits")
  emit_hits "Blocking async bridge (block_in_place / block_on) found" "$filtered_block_bridge"

  # Flag unbounded channels (prefer bounded or coalescing queues).
  unbounded_hits=$(rg --no-heading "mpsc::unbounded_channel\\(|async_channel::unbounded\\(|mpsc::unbounded\\(" crates -g "*.rs" || true)
  filtered_unbounded=$(filter_common_allowlist "$unbounded_hits")
  emit_hits "Unbounded channel usage (prefer bounded/coalescing)" "$filtered_unbounded"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_REACTIVE" = true ]; then
  section "Reactive data model — signals are source of truth; no domain data in props; components subscribe to signals (docs/115_cli_tui.md)"

  # Check 1: Props structs with explicit "Domain data" comments
  # This catches the current pattern where domain data is explicitly marked in props
  domain_data_in_props=$(rg --no-heading -l "// === Domain data" crates/aura-terminal/src/tui/screens -g "*.rs" || true)
  if [ -n "$domain_data_in_props" ]; then
    # Count unique files with domain data in props
    file_count=$(echo "$domain_data_in_props" | wc -l | tr -d ' ')
    violation "[L7] Domain data in props: $file_count screen(s) pass domain data as props instead of subscribing to signals"
    echo -e "    ${YELLOW}Files:${NC}"
    echo "$domain_data_in_props" | while read -r f; do
      [ -n "$f" ] && echo "      - $f"
    done
    echo -e "    ${YELLOW}Fix:${NC} Remove domain data fields from *ScreenProps; subscribe to signals in component"
    echo -e "    ${YELLOW}Ref:${NC} docs/115_cli_tui.md §Reactive data model"
  else
    info "Domain data in props: none (all screens use signal subscriptions)"
  fi

  # Check 2: Known domain types in Props structs that should come from signals
  # These types are domain data that should be subscribed to, not passed as props
  domain_types="Vec<Contact>|Vec<Channel>|Vec<Message>|Vec<Guardian>|Vec<Device>|Vec<Resident>|Vec<BlockSummary>|Vec<PendingRequest>"

  # Find Props structs containing domain types
  # Use multiline matching to find struct definitions with these types
  domain_type_hits=$(rg --no-heading "$domain_types" crates/aura-terminal/src/tui/screens -g "*screen.rs" || true)
  # Filter to only Props structs (lines near "Props" or containing "pub ")
  filtered_domain_types=$(echo "$domain_type_hits" | grep -E "pub |ScreenProps" | grep -v "// Subscribe" | grep -v "use_state" || true)

  if [ -n "$filtered_domain_types" ]; then
    type_count=$(echo "$filtered_domain_types" | wc -l | tr -d ' ')
    if [ "$type_count" -gt 0 ]; then
      verbose "Domain types in screen files (may be in Props or local state):"
      echo "$filtered_domain_types" | head -5 | while read -r line; do
        verbose "  $line"
      done
    fi
  fi

  # Check 3: Screen components that don't subscribe to any signals
  # Each screen.rs should have at least one subscribe_signal_with_retry call
  screens_dir="crates/aura-terminal/src/tui/screens"
  if [ -d "$screens_dir" ]; then
    missing_subscriptions=""
    for screen_file in $(find "$screens_dir" -name "screen.rs" 2>/dev/null); do
      # Check if the file has a signal subscription
      if ! grep -q "subscribe_signal_with_retry\|SIGNAL" "$screen_file" 2>/dev/null; then
        missing_subscriptions="${missing_subscriptions}${screen_file}"$'\n'
      fi
    done
    if [ -n "$missing_subscriptions" ]; then
      emit_hits "Screen without signal subscription (should subscribe to domain signals)" "$missing_subscriptions"
    else
      info "All screens subscribe to signals"
    fi
  fi

  verbose "Reactive pattern: Props should only contain view state (focus, selection), callbacks, and configuration"
  verbose "Domain data (contacts, messages, guardians, etc.) should come from signal subscriptions"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_UI" = true ]; then
  section "UI boundary — aura-terminal uses aura_app::ui facade; no direct protocol/journal access"

  direct_app_modules=$(rg --no-heading "aura_app::(workflows|signal_defs|views|runtime_bridge|authorization)" crates/aura-terminal/src -g "*.rs" || true)
  filtered_app_modules=$(echo "$direct_app_modules" | grep -v "///" | grep -v "//" || true)
  emit_hits "Direct aura_app module access in aura-terminal (use aura_app::ui::* facade)" "$filtered_app_modules"

  view_access_hits=$(rg --no-heading "\\.views\\(" crates/aura-terminal/src -g "*.rs" || true)
  emit_hits "Direct ViewState access in aura-terminal (use signals)" "$view_access_hits"

  journal_hits=$(rg --no-heading "FactRegistry|FactReducer|RelationalFact|JournalEffects|commit_.*facts|RuntimeBridge::commit" crates/aura-terminal/src -g "*.rs" || true)
  emit_hits "Direct journal/protocol mutation in aura-terminal (use workflows)" "$journal_hits"

  forbidden_crate_hits=$(rg --no-heading "aura_(journal|protocol|consensus|guards|amp|anti_entropy|transport|recovery|sync|invitation|authentication|relational|chat)::" crates/aura-terminal/src -g "*.rs" || true)
  emit_hits "Direct protocol/domain crate usage in aura-terminal (use aura_app::ui facade)" "$forbidden_crate_hits"

  section "Terminal time — use algebraic effects (PhysicalTimeEffects), no OS time"

  terminal_time_hits=$(rg --no-heading -n "SystemTime::now|Instant::now|std::time::Instant|std::time::SystemTime|chrono::Utc::now|chrono::Local::now" crates/aura-terminal/src -g "*.rs" || true)
  filtered_terminal_time=$(echo "$terminal_time_hits" \
    | grep -v "///" \
    | grep -v "//!" \
    | grep -v "//" || true)
  emit_hits "Direct OS time usage in aura-terminal (use PhysicalTimeEffects)" "$filtered_terminal_time"

  section "Terminal business logic — validation and domain state should be in aura_app::workflows"

  # Check for threshold validation logic in terminal (should use workflows::account)
  # Filter: threshold vs num_devices/configs.len comparisons (domain validation)
  # Exclude: progress() calculations, UI-only checks, tests, and lines using workflow
  threshold_validation=$(rg --no-heading "threshold\s*(>|<|==|!=)\s*(num_devices|configs\.len|0)" crates/aura-terminal/src -g "*.rs" || true)
  filtered_threshold=""
  if [ -n "$threshold_validation" ]; then
    while IFS= read -r hit; do
      [ -z "$hit" ] && continue
      file="${hit%%:*}"
      # Check if previous line has UI/division/guard comment
      line_content="${hit#*:}"
      line_num=$(grep -n "$line_content" "$file" 2>/dev/null | head -1 | cut -d: -f1)
      if [ -n "$line_num" ] && [ "$line_num" -gt 1 ]; then
        prev_line=$((line_num - 1))
        prev_content=$(sed -n "${prev_line}p" "$file" 2>/dev/null)
        # Skip if previous line has UI/progress/division comment
        if echo "$prev_content" | grep -qiE "// (UI|progress|Division|guard|not domain)"; then
          continue
        fi
      fi
      # Skip if line itself has workflow/test markers
      if echo "$hit" | grep -qE "(uses workflow|workflows::account|/tests/)"; then
        continue
      fi
      filtered_threshold="${filtered_threshold}${hit}"$'\n'
    done <<< "$threshold_validation"
  fi
  emit_hits "Threshold validation in terminal (use aura_app::ui::workflows::account)" "$filtered_threshold"

  # Check for local domain state (HashSet/HashMap of peers, guardians, etc)
  # These should use AppCore signals instead
  local_domain_state=$(rg --no-heading "HashSet<.*Id>|HashMap<.*Id," crates/aura-terminal/src/handlers -g "*.rs" || true)
  filtered_domain_state=$(echo "$local_domain_state" \
    | grep -v "// temporary" \
    | grep -v "// local cache" \
    | grep -v "/tests/" || true)
  emit_hits "Local domain state in terminal handlers (use AppCore signals)" "$filtered_domain_state"

  # Check for guardian count validation in terminal (should be in workflows)
  guardian_validation=$(rg --no-heading "guardians?\.(len|count|is_empty)\(\)\s*(>|<|==|>=|<=)" crates/aura-terminal/src -g "*.rs" || true)
  filtered_guardian=$(echo "$guardian_validation" \
    | grep -v "// uses workflow" \
    | grep -v "/tests/" || true)
  if [ -n "$filtered_guardian" ]; then
    emit_hits "Guardian count validation in terminal (consider moving to workflow)" "$filtered_guardian"
  fi

  verbose "Terminal layer should only handle I/O and formatting; all business logic belongs in aura_app::workflows"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_WORKFLOWS" = true ]; then
  section "Workflow hygiene — use helpers for runtime access, parsing, and signals"

  runtime_string_hits=$(rg --no-heading "Runtime bridge not available" crates/aura-app/src/workflows -g "*.rs" || true)
  filtered_runtime_string_hits=$(echo "$runtime_string_hits" \
    | grep -v "crates/aura-app/src/workflows/runtime.rs" \
    | grep -v '\.contains(' || true)
  emit_hits "Direct runtime error strings in workflows (use workflows::runtime::require_runtime for consistent errors + wiring)" "$filtered_runtime_string_hits"

  parse_authority_hits=$(rg --no-heading "parse::<AuthorityId>" crates/aura-app/src/workflows -g "*.rs" || true)
  filtered_parse_authority_hits=$(echo "$parse_authority_hits" | grep -v "crates/aura-app/src/workflows/parse.rs" || true)
  emit_hits "Direct AuthorityId parsing in workflows (use workflows::parse::parse_authority_id for normalized errors)" "$filtered_parse_authority_hits"

  parse_context_hits=$(rg --no-heading "parse::<ContextId>" crates/aura-app/src/workflows -g "*.rs" || true)
  filtered_parse_context_hits=$(echo "$parse_context_hits" | grep -v "crates/aura-app/src/workflows/parse.rs" || true)
  emit_hits "Direct ContextId parsing in workflows (use workflows::parse::parse_context_id for normalized errors)" "$filtered_parse_context_hits"

  signal_access_hits=$(rg --no-heading "\\.(read|emit)\\(&\\*.*_SIGNAL" crates/aura-app/src/workflows -g "*.rs" || true)
  filtered_signal_access_hits=$(echo "$signal_access_hits" | grep -v "crates/aura-app/src/workflows/signals.rs" || true)
  emit_hits "Direct signal access in workflows (use workflows::signals::{read_signal, emit_signal} + signal_defs::*_SIGNAL_NAME constants)" "$filtered_signal_access_hits"

  init_signals_hits=$(rg --no-heading "init_signals\\(" crates/aura-app/src -g "*.rs" || true)
  filtered_init_signals_hits=$(echo "$init_signals_hits" \
    | grep -v "crates/aura-app/src/core/app.rs" \
    | grep -v "init_signals_with_hooks" || true)
  emit_hits "Direct init_signals calls (use AppCore::init_signals_with_hooks to attach workflow hooks)" "$filtered_init_signals_hits"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_SERIALIZATION" = true ]; then
  section "Serialization — use DAG-CBOR (aura_core::util::serialization) for all wire protocols and facts; no bincode anywhere"

  # Check for bincode usage - no allowlist, bincode should not be used anywhere
  bincode_hits=$(rg --no-heading "bincode::" crates -g "*.rs" || true)
  filtered_bincode=$(echo "$bincode_hits" \
    | grep -v "/examples/" \
    | grep -v "benches/" || true)

  if [ -n "$filtered_bincode" ]; then
    emit_hits "bincode usage (migrate to aura_core::util::serialization)" "$filtered_bincode"
    echo -e "    ${YELLOW}Migration:${NC} bincode::serialize → to_vec, bincode::deserialize → from_slice"
    echo -e "    ${YELLOW}Canonical import:${NC} use aura_core::util::serialization::{to_vec, from_slice, hash_canonical};"
    echo -e "    ${YELLOW}Ref:${NC} work/024.md (serialization migration)"
  else
    info "bincode usage: none outside testkit"
  fi

  # Check that wire protocol files use canonical serialization
  # Wire protocol files (wire.rs, protocol messages) should use DAG-CBOR
  wire_files=$(find crates -type f \( -name "wire.rs" -o -name "*_wire.rs" \) 2>/dev/null || true)
  non_canonical_wire=""
  for wire_file in $wire_files; do
    # Check if file uses canonical serialization or has no serialization at all
    if grep -q "serde_json::to_vec\|serde_json::from_slice\|bincode::" "$wire_file" 2>/dev/null; then
      if ! grep -q "aura_core::util::serialization\|crate::util::serialization" "$wire_file" 2>/dev/null; then
        non_canonical_wire="${non_canonical_wire}${wire_file}"$'\n'
      fi
    fi
  done
  if [ -n "$non_canonical_wire" ]; then
    emit_hits "Wire protocol without canonical DAG-CBOR serialization" "$non_canonical_wire"
  else
    info "Wire protocols: using canonical serialization"
  fi

  # Check facts.rs files for proper versioned serialization
  facts_files=$(find crates -path "*/src/facts.rs" -type f 2>/dev/null | grep -v aura-core || true)
  non_versioned_facts=""
  for facts_file in $facts_files; do
    # Facts files should have VersionedMessage or use canonical serialization
    if grep -q "Serialize\|Deserialize" "$facts_file" 2>/dev/null; then
      if ! grep -qE "aura_core::util::serialization|Versioned.*Fact|from_slice|to_vec" "$facts_file" 2>/dev/null; then
        non_versioned_facts="${non_versioned_facts}${facts_file}"$'\n'
      fi
    fi
  done
  if [ -n "$non_versioned_facts" ]; then
    emit_hits "Facts file without versioned DAG-CBOR serialization" "$non_versioned_facts"
  else
    info "Facts files: using versioned serialization"
  fi

  verbose "Canonical serialization: aura_core::util::serialization::{to_vec, from_slice, hash_canonical}"
  verbose "Allowed alternatives: serde_json for debug output, config files, dynamic metadata"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_EFFECTS" = true ]; then
  section "Identifier invariants — deterministic SessionId::new() is test-only"

  session_id_hits=$(rg --no-heading "SessionId::new\\(" crates -g "*.rs" || true)
  filtered_session_id=$(echo "$session_id_hits" \
    | grep -v "/tests/" \
    | grep -v "/benches/" \
    | grep -v "/examples/" \
    | grep -v "cfg(test)" \
    | grep -v "cfg\\(test\\)" || true)

  if [ -n "$filtered_session_id" ]; then
    emit_hits "SessionId::new() used outside tests (use SessionId::new_from_entropy / RandomEffects)" "$filtered_session_id"
  else
    info "SessionId::new(): none outside tests"
  fi
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_STYLE" = true ]; then
  section "Rust Style Guide — safety and API rules (work/030.md)"

  # Safety §3: "Prefer explicitly-sized integers, avoid usize in stored formats"
  # Find structs with Serialize/Deserialize that contain usize fields
  # Use -U for multiline matching to catch struct definitions spanning lines
  usize_serialized=$(rg --no-heading -n "usize" crates -g "*.rs" \
    | xargs -I{} sh -c 'file="${1%%:*}"; if grep -l "#\[derive.*Serialize" "$file" >/dev/null 2>&1; then echo "$1"; fi' _ {} 2>/dev/null || true)
  filtered_usize=$(echo "$usize_serialized" \
    | grep -v "/tests/" \
    | grep -v "/benches/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "// usize ok:" \
    | grep -v "fn " \
    | grep -v "let " \
    | grep -v "for " \
    | grep -v "impl " || true)
  # Only show field definitions (pub xxx: usize or xxx: usize,)
  field_usize=$(echo "$filtered_usize" | grep -E ":\s*usize\s*[,}]|pub\s+\w+:\s*usize" || true)
  emit_hits "usize in serialized struct field (use u32/u64 for wire formats; Safety §3)" "$field_usize"
  verbose "Add '// usize ok: <reason>' comment to suppress false positives"

  # Safety §2: "Every queue, buffer, batch, map must have a hard upper bound"
  # Find Vec<u8> fields in core types (signatures, payloads, ciphertext) without MAX_* constants
  unbounded_bytes=$(rg --no-heading -n "pub\s+\w+:\s*Vec<u8>" crates/aura-core/src -g "*.rs" || true)
  if [ -n "$unbounded_bytes" ]; then
    missing_bounds=""
    while IFS= read -r hit; do
      [ -z "$hit" ] && continue
      file="${hit%%:*}"
      # Check if file has a MAX_*_BYTES or MAX_*_SIZE constant
      if ! grep -qE "const\s+MAX_.*_(BYTES|SIZE|LEN)" "$file" 2>/dev/null; then
        missing_bounds="${missing_bounds}${hit}"$'\n'
      fi
    done <<< "$unbounded_bytes"
    emit_hits "Vec<u8> field without MAX_*_BYTES constant in same file (Safety §2)" "$missing_bounds"
  else
    info "Vec<u8> bounds: all core types have MAX_* constants"
  fi

  # Safety §2: "Encode limits as constants with units in the name"
  # Find numeric constants without unit suffixes
  constants_no_units=$(rg --no-heading -n "const\s+[A-Z][A-Z0-9_]+:\s*(u\d+|i\d+|usize)\s*=\s*\d+" crates/aura-core/src -g "*.rs" \
    | grep -vE "_(MS|BYTES|COUNT|SIZE|MAX|MIN|LEN|LIMIT|DEPTH|HEIGHT|BITS|SECS|NANOS)(\s*:|:)" \
    | grep -vE "VERSION|MAGIC|EPOCH|THRESHOLD|FACTOR|RATIO|WIRE_FORMAT|DEFAULT_" \
    | grep -v "/tests/" \
    | grep -v "/benches/" || true)
  if [ -n "$constants_no_units" ]; then
    emit_hits "Numeric constant without unit suffix (_MS, _BYTES, _COUNT, etc.; Safety §2)" "$constants_no_units"
    verbose "Expected patterns: TIMEOUT_MS, BATCH_SIZE_MAX, MAX_RETRY_COUNT, BUFFER_SIZE_BYTES"
  else
    info "Constants with units: all numeric constants have unit suffixes"
  fi

  # Style by Numbers: "#[must_use] for APIs where dropping the value is likely a bug"
  # Find builder methods (with_*) without #[must_use]
  builder_methods=$(rg --no-heading -n "pub\s+(const\s+)?fn\s+with_\w+\s*\(" crates/aura-core/src -g "*.rs" || true)
  if [ -n "$builder_methods" ]; then
    missing_must_use=""
    while IFS= read -r hit; do
      [ -z "$hit" ] && continue
      file="${hit%%:*}"
      rest="${hit#*:}"
      linenum="${rest%%:*}"
      # Check if previous 1-3 lines have #[must_use]
      has_must_use=false
      for offset in 1 2 3; do
        prev_line=$((linenum - offset))
        if [ "$prev_line" -gt 0 ]; then
          if sed -n "${prev_line}p" "$file" 2>/dev/null | grep -q "#\[must_use\]"; then
            has_must_use=true
            break
          fi
        fi
      done
      if [ "$has_must_use" = false ]; then
        missing_must_use="${missing_must_use}${hit}"$'\n'
      fi
    done <<< "$builder_methods"
    emit_hits "Builder method without #[must_use] (Style by Numbers)" "$missing_must_use"
  else
    info "Builder methods: all have #[must_use]"
  fi

  info "Rust style guide checks complete (see also: clippy lints in Cargo.toml)"
fi

if [ "$RUN_ALL" = true ] || [ "$RUN_TODOS" = true ]; then
  section "Production placeholders — replace nil UUIDs/placeholder implementations with real IDs/derivations (see docs/105_identifiers_and_boundaries.md, docs/001_system_architecture.md §1.4)"
  # Note: "placeholder" in UI code (input hints, props.placeholder) is intentional and not a violation
  # Only flag "placeholder implementation" and Uuid::nil() which indicate incomplete code
  placeholder_hits=$(rg --no-heading -i "uuid::nil\\(\\)|placeholder implementation" crates -g "*.rs" \
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
  # PLATFORM_BUILDERS_ALLOWLIST: Platform preset builders (android.rs, ios.rs, web.rs) contain
  # explicit TODOs for future platform handlers. These are behind feature flags and already
  # return proper errors explaining the platform isn't implemented. Not architecture blockers.
  PLATFORM_BUILDERS_ALLOWLIST="crates/aura-agent/src/builder/android.rs|crates/aura-agent/src/builder/ios.rs|crates/aura-agent/src/builder/web.rs"
  # TUI_FEATURE_ALLOWLIST: L7 UI feature work for callback implementation and modal state
  # propagation. These are tracked enhancement items, not architecture violations:
  # - shell.rs: channel deletion, contact removal, invitation revocation callbacks
  # - state_machine.rs: passing selected channel info to modals
  TUI_FEATURE_ALLOWLIST="Implement channel deletion callback|Implement contact removal callback|Implement invitation revocation callback|Pass actual channel"
  todo_hits=$(rg --no-heading "TODO|FIXME" crates -g "*.rs" \
    | grep -v "/tests/" \
    | grep -v "/benches/" \
    | grep -v "/examples/" \
    | grep -vE "$PLATFORM_BUILDERS_ALLOWLIST" \
    | grep -vE "$TUI_FEATURE_ALLOWLIST" || true)
  emit_hits "TODO/FIXME" "$todo_hits"

  section "Incomplete markers — replace \"in production\"/WIP text with TODOs or complete implementation per docs/805_development_patterns.md"
  incomplete_pattern="in production[^\\n]*(would|should|not)|stub|not implemented|unimplemented|temporary|workaround|hacky|\\bWIP\\b|\\bTBD\\b|prototype|future work|to be implemented"
  incomplete_hits=$(rg --no-heading -i "$incomplete_pattern" crates -g "*.rs" || true)
  # INTENTIONAL_STUBS_ALLOWLIST: Documented development/testing APIs that are intentionally "stubs"
  # - biscuit_capability_stub: Explicit fallback API for Biscuit capability checking when
  #   RuntimeBridge isn't available. Documented in dispatcher.rs with clear integration guidance.
  #   All related comments (Stub implementation, In production, allowed in stub, etc.) are part
  #   of this documented API and not incomplete code.
  # - "in production this would be": Accurate description of placeholder behavior in workflows
  #   that will be updated when user identity propagation is implemented.
  INTENTIONAL_STUBS_ALLOWLIST="biscuit_capability_stub|in production this would be the actual|effects/dispatcher.rs.*[Ss]tub|effects/dispatcher.rs.*[Ii]n production"
  # Filter out tests, benches, examples, bin/ directories, and intentional stubs
  filtered_incomplete=$(echo "$incomplete_hits" \
    | grep -v "/tests/" \
    | grep -v "/benches/" \
    | grep -v "/examples/" \
    | grep -v "/bin/" \
    | grep -vE "$INTENTIONAL_STUBS_ALLOWLIST" \
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
  if [ "$VERBOSE" = true ] && [ ${#VIOLATION_DETAILS[@]} -gt 0 ]; then
    echo -e "\n${BOLD}Violation details:${NC}"
    for detail in "${VIOLATION_DETAILS[@]}"; do
      echo "  - $detail"
    done
  fi
fi

# Show quick mode hint if many violations
if [ $VIOLATIONS -gt 10 ] && [ "$RUN_QUICK" = false ]; then
  echo -e "\n${YELLOW}Tip:${NC} Use --quick to skip TODO/placeholder checks for faster iteration"
fi

exit $([ $VIOLATIONS -eq 0 ] && echo 0 || echo 1)
