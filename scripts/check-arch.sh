#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# Aura Architectural Compliance Checker
# ═══════════════════════════════════════════════════════════════════════════════

set -euo pipefail

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

Usage: scripts/check-arch.sh [OPTIONS]

Options (run all when none given):
  --layers         Layer boundary and purity checks
  --deps           Dependency direction checks
  --effects        Effect placement and handler sanity
  --guards         Guard-chain bypass heuristics
  --invariants     INVARIANTS.md schema validation
  --todos          Incomplete code markers
  --registration   Handler composition vs direct instantiation
  --crypto         Crypto library usage boundaries
  --concurrency    Concurrency hygiene (block_in_place, unbounded channels)
  --reactive       TUI reactive data model
  --ui             UI boundary checks
  --workflows      aura-app workflow hygiene
  --serialization  Serialization format enforcement
  --style          Rust style guide rules
  --layer N[,M...] Filter to specific layers (1-8)
  --quick          Skip slow checks (todos, placeholders)
  -v, --verbose    Show more detail
  -h, --help       Show this help
EOF
}

# ───────────────────────────────────────────────────────────────────────────────
# Flag Parsing
# ───────────────────────────────────────────────────────────────────────────────
RUN_ALL=true VERBOSE=false RUN_QUICK=false
RUN_LAYERS=false RUN_DEPS=false RUN_EFFECTS=false RUN_GUARDS=false
RUN_INVARIANTS=false RUN_TODOS=false RUN_REG=false RUN_CRYPTO=false
RUN_CONCURRENCY=false RUN_REACTIVE=false RUN_UI=false RUN_WORKFLOWS=false
RUN_SERIALIZATION=false RUN_STYLE=false
LAYER_FILTERS=()

while [[ $# -gt 0 ]]; do
  case $1 in
    --layers)        RUN_ALL=false; RUN_LAYERS=true ;;
    --deps)          RUN_ALL=false; RUN_DEPS=true ;;
    --effects)       RUN_ALL=false; RUN_EFFECTS=true ;;
    --guards)        RUN_ALL=false; RUN_GUARDS=true ;;
    --invariants)    RUN_ALL=false; RUN_INVARIANTS=true ;;
    --todos)         RUN_ALL=false; RUN_TODOS=true ;;
    --registration)  RUN_ALL=false; RUN_REG=true ;;
    --crypto)        RUN_ALL=false; RUN_CRYPTO=true ;;
    --concurrency)   RUN_ALL=false; RUN_CONCURRENCY=true ;;
    --reactive)      RUN_ALL=false; RUN_REACTIVE=true ;;
    --ui)            RUN_ALL=false; RUN_UI=true ;;
    --workflows)     RUN_ALL=false; RUN_WORKFLOWS=true ;;
    --serialization) RUN_ALL=false; RUN_SERIALIZATION=true ;;
    --style)         RUN_ALL=false; RUN_STYLE=true ;;
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
  RUN_LAYERS=true RUN_DEPS=true RUN_EFFECTS=true RUN_GUARDS=true
  RUN_INVARIANTS=true RUN_REG=true RUN_CRYPTO=true RUN_CONCURRENCY=true
  RUN_REACTIVE=true RUN_SERIALIZATION=true RUN_STYLE=true RUN_WORKFLOWS=true
  RUN_TODOS=false  # Skip in quick mode
fi

# ───────────────────────────────────────────────────────────────────────────────
# Allowlists (paths that legitimately need impure/direct operations)
# ───────────────────────────────────────────────────────────────────────────────
# Layer 3: Infrastructure effect implementations
ALLOW_EFFECTS="crates/aura-effects/src/"
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
violation() { ((VIOLATIONS++)) || true; VIOLATION_DETAILS+=("$1"); echo -e "${RED}✖${NC} $1"; }
warning()   { violation "$1"; }  # Warnings treated as violations for strict compliance
info()      { echo -e "${BLUE}•${NC} $1"; }
section()   { echo -e "\n${BOLD}${CYAN}$1${NC}"; }
verbose()   { $VERBOSE && echo -e "${BLUE}  ↳${NC} $1" || true; }
hint()      { echo -e "    ${YELLOW}Fix:${NC} $1"; }

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

get_layer_from_path() {
  local crate
  crate=$(echo "$1" | sed 's|^crates/||' | cut -d/ -f1)
  layer_of "$crate"
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


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Layer Purity
# ═══════════════════════════════════════════════════════════════════════════════
check_layers() {
  section "Layer purity — aura-core interface-only; impls in aura-effects or domain crates"

  # aura-core should only define traits/types (no impl of Effects)
  if grep -RE "\bimpl\b.*Effects" crates/aura-core/src 2>/dev/null | grep -v "trait" | grep -v "impl<" | grep -v ":///" >/dev/null; then
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
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Dependency Direction
# ═══════════════════════════════════════════════════════════════════════════════
check_deps() {
  section "Dependency direction — no upward deps (Lx→Ly where y>x)"

  if check_cargo; then
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
  # ─── Infrastructure effect trait placement ───
  section "Effects — infra traits in aura-core; impls in aura-effects; mocks in aura-testkit"

  local infra_traits="CryptoEffects|NetworkEffects|StorageEffects|PhysicalTimeEffects|LogicalClockEffects|OrderClockEffects|TimeAttestationEffects|RandomEffects|ConsoleEffects|ConfigurationEffects|LeakageEffects"
  local infra_defs
  infra_defs=$(find crates/ -name "*.rs" -not -path "*/aura-core/*" -exec grep -El "pub trait ($infra_traits)" {} + 2>/dev/null || true)
  if [[ -n "$infra_defs" ]]; then
    violation "Infrastructure effect traits defined outside aura-core:"
    echo "$infra_defs"
  else
    info "Infra effect traits: only in aura-core"
  fi

  # Stateful constructs in aura-effects (should be stateless)
  local stateful
  stateful=$(grep -R "Arc<Mutex\|Arc<RwLock\|Rc<RefCell" crates/aura-effects/src 2>/dev/null | grep -v "test" | grep -v "reactive/handler.rs" | grep -v "query/handler.rs" || true)
  [[ -n "$stateful" ]] && { violation "aura-effects contains stateful constructs"; echo "$stateful"; }

  # Mock handlers in wrong location
  grep -R "Mock.*Handler\|InMemory.*Handler" crates/aura-effects/src 2>/dev/null | grep -v "test" >/dev/null && \
    violation "Mock handlers in aura-effects (should be in aura-testkit)"

  # Infrastructure effects outside aura-effects
  local infra_impls
  infra_impls=$(rg --no-heading --glob "*.rs" "impl\s+[^\n{}]*for[^\n{}]*(CryptoEffects|NetworkEffects|StorageEffects|PhysicalTimeEffects|LogicalClockEffects|OrderClockEffects|TimeAttestationEffects|RandomEffects|ConsoleEffects|ConfigurationEffects)" crates \
    | grep -v "crates/aura-effects/" | grep -v "crates/aura-testkit/" | grep -v "crates/aura-core/" | grep -v "tests/" || true)
  emit_hits "Infrastructure effects outside aura-effects" "$infra_impls"

  # Application effects in aura-effects
  local app_effects="JournalEffects|AuthorityEffects|FlowBudgetEffects|AuthorizationEffects|RelationalContextEffects|GuardianEffects|ChoreographicEffects|EffectApiEffects|SyncEffects"
  local app_impls
  app_impls=$(grep -R "impl.*\($app_effects\)" crates/aura-effects/src 2>/dev/null | grep -v "test" || true)
  [[ -n "$app_impls" ]] && violation "Application effects in aura-effects (should be in domain crates)" || info "No app effects in aura-effects"

  # ─── Direct OS operations ───
  section "Direct OS operations — use effect traits instead"

  # std::fs usage
  local fs_hits filtered_fs
  fs_hits=$(rg --no-heading "std::fs::|std::io::File|std::io::BufReader|std::io::BufWriter" crates -g "*.rs" || true)
  filtered_fs=$(filter_allow "$fs_hits" "$ALLOW_RUNTIME|$ALLOW_APP_NATIVE|$ALLOW_TUI_BOOTSTRAP|$ALLOW_TUI_INFRA")
  filtered_fs=$(filter_test_modules "$filtered_fs")
  emit_hits "Direct std::fs (use StorageEffects)" "$filtered_fs"

  # std::net usage
  local net_hits filtered_net
  net_hits=$(rg --no-heading "std::net::|TcpStream|TcpListener|UdpSocket" crates -g "*.rs" || true)
  filtered_net=$(filter_allow "$net_hits" "$ALLOW_RUNTIME")
  emit_hits "Direct std::net (use NetworkEffects)" "$filtered_net"

  # ─── Runtime coupling ───
  section "Runtime coupling — wrap tokio/async-std behind effects"

  local runtime_hits filtered_runtime
  runtime_hits=$(rg --no-heading -n "tokio::|async_std::" crates -g "*.rs" || true)
  filtered_runtime=$(echo "$runtime_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-agent/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-terminal/" \
    | grep -v "crates/aura-composition/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-macros/" \
    | grep -Ev "$ALLOW_APP_NATIVE" \
    | grep -v "crates/aura-authorization/src/storage_authorization.rs" \
    | grep -v "crates/aura-core/src/effects/reactive.rs" \
    | grep -v "#\\[tokio::test\\]" \
    | grep -v "#\\[async_std::test\\]" \
    | grep -v "#\\[tokio::main\\]" \
    | grep -Ev "/tests/|/examples/|benches/" || true)
  filtered_runtime=$(filter_test_modules "$filtered_runtime")
  emit_hits "Runtime usage outside handler layers" "$filtered_runtime"

  # aura-app should be runtime-agnostic
  section "aura-app runtime-agnostic surface"
  local app_runtime
  app_runtime=$(rg --no-heading -n "tokio::|async_std::" crates/aura-app/src -g "*.rs" \
    | grep -v "#\\[tokio::test\\]" | grep -v "#\\[async_std::test\\]" | grep -Ev "/tests/|/benches/" || true)
  emit_hits "tokio/async-std in aura-app" "$app_runtime"

  # ─── Impure functions ───
  section "Impure functions — route through effect traits"

  local impure_pattern="SystemTime::now|Instant::now|thread_rng\\(|rand::thread_rng|chrono::Utc::now|chrono::Local::now|rand::rngs::OsRng|rand::random"
  local impure_hits filtered_impure
  impure_hits=$(rg --no-heading "$impure_pattern" crates -g "*.rs" || true)
  filtered_impure=$(echo "$impure_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -Ev "$ALLOW_RUNTIME" \
    | grep -v "crates/aura-terminal/" \
    | grep -Ev "/tests/|/benches/" \
    | grep -v "///" | grep -v "//!" | grep -v "//" \
    | grep -v "\.unwrap()" | grep -v "#\[tokio::test\]" | grep -v "#\[test\]" || true)
  filtered_impure=$(filter_test_modules "$filtered_impure")
  emit_hits "Impure functions outside effect handlers" "$filtered_impure"

  # ─── Physical time guardrails ───
  section "Physical time — use PhysicalTimeEffects::sleep_ms"

  local tokio_sleep filtered_sleep
  tokio_sleep=$(rg --no-heading -n "tokio::time::sleep" crates -g "*.rs" || true)
  filtered_sleep=$(echo "$tokio_sleep" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-terminal/" \
    | grep -Ev "/tests/|/examples/|benches/" || true)
  filtered_sleep=$(filter_test_modules "$filtered_sleep")
  emit_hits "Direct tokio::time::sleep" "$filtered_sleep"

  local std_sleep
  std_sleep=$(rg --no-heading "std::thread::sleep|async_std::task::sleep" crates -g "*.rs" \
    | grep -v "crates/aura-effects/src/time.rs" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-testkit/" \
    | grep -Ev "/tests/|benches/" || true)
  emit_hits "Direct std/async-std sleep" "$std_sleep"

  # aura-sync runtime neutrality
  section "aura-sync runtime neutrality"
  local sync_runtime
  sync_runtime=$(rg --no-heading -n "tokio::|async_std::" crates/aura-sync/src/protocols -g "*.rs" | grep -v "///" | grep -v "//!" | grep -v "//" || true)
  [[ -n "$sync_runtime" ]] && emit_hits "Runtime in aura-sync protocols" "$sync_runtime" || info "aura-sync protocols: runtime-neutral"

  # ─── Simulation control surfaces ───
  section "Simulation surfaces — inject via effects"

  local sim_pattern="rand::random|rand::thread_rng|rand::rngs::OsRng|RngCore::fill_bytes|std::io::stdin|read_line\\(|std::thread::spawn"
  local sim_hits filtered_sim
  sim_hits=$(rg --no-heading "$sim_pattern" crates -g "*.rs" || true)
  filtered_sim=$(echo "$sim_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-agent/src/runtime/" \
    | grep -v "$ALLOW_TUI_BOOTSTRAP" \
    | grep -Ev "/tests/" \
    | grep -v "///" | grep -v "//!" | grep -v "//" || true)
  emit_hits "Non-injected randomness/IO/spawn" "$filtered_sim"

  # ─── Pure interpreter alignment ───
  section "Pure interpreter — migrate to GuardSnapshot + EffectCommand"

  local guard_sync
  guard_sync=$(rg --no-heading "GuardEffectSystem|futures::executor::block_on" crates -g "*.rs" | sort -u || true)
  emit_hits "Synchronous guard/effect bridges" "$guard_sync"

  # ─── Identifier determinism ───
  section "Identifier determinism — avoid entropy-consuming IDs"

  local entropy_pattern="AuthorityId::new\\(\\)|ContextId::new\\(\\)|DeviceId::new\\(\\)"
  local entropy_hits filtered_entropy
  entropy_hits=$(rg --no-heading "$entropy_pattern" crates -g "*.rs" || true)
  filtered_entropy=$(echo "$entropy_hits" | grep -v "$ALLOW_EFFECTS" | grep -Ev "$ALLOW_RUNTIME" | grep -v "$ALLOW_CLI" | grep -Ev "$ALLOW_TESTS" || true)
  if [[ -n "$filtered_entropy" ]]; then
    local sorted
    sorted=$(echo "$filtered_entropy" | sort_by_layer)
    while IFS= read -r entry; do
      [[ -z "$entry" ]] && continue
      local layer="${entry:0:2}" content="${entry:3}"
      [[ "$layer" == "99" ]] && layer="?"
      layer="${layer#0}"
      layer_filter_matches "$layer" || continue
      violation "[L${layer}] Entropy-consuming ID: $content"
      hint "Use XxxId::new_from_entropy([n; 32]) or from_uuid(Uuid::from_bytes([n; 16]))"
    done <<< "$sorted"
  else
    info "Entropy-consuming IDs: none"
  fi

  # Uuid::new_v4
  local uuid_hits filtered_uuid
  uuid_hits=$(rg --no-heading "Uuid::new_v4|uuid::Uuid::new_v4" crates -g "*.rs" || true)
  filtered_uuid=$(echo "$uuid_hits" | grep -v "$ALLOW_EFFECTS" | grep -Ev "$ALLOW_RUNTIME" | grep -v "$ALLOW_CLI" | grep -Ev "$ALLOW_TESTS" || true)
  if [[ -n "$filtered_uuid" ]]; then
    local sorted
    sorted=$(echo "$filtered_uuid" | sort_by_layer)
    while IFS= read -r entry; do
      [[ -z "$entry" ]] && continue
      local layer="${entry:0:2}" content="${entry:3}"
      [[ "$layer" == "99" ]] && layer="?"
      layer="${layer#0}"
      layer_filter_matches "$layer" || continue
      violation "[L${layer}] Entropy-consuming UUID: $content"
      hint "Use Uuid::nil() or Uuid::from_bytes([n; 16])"
    done <<< "$sorted"
  else
    info "Entropy-consuming UUIDs: none"
  fi

  # Direct rand usage
  local rand_hits filtered_rand
  rand_hits=$(rg --no-heading "rand::random|thread_rng\\(\\)|rand::thread_rng" crates -g "*.rs" || true)
  filtered_rand=$(echo "$rand_hits" \
    | grep -v "crates/aura-effects/" \
    | grep -v "crates/aura-testkit/" \
    | grep -v "crates/aura-simulator/" \
    | grep -v "crates/aura-agent/src/runtime/" \
    | grep -Ev "/tests/" \
    | grep -v "///" | grep -v "//!" || true)
  if [[ -n "$filtered_rand" ]]; then
    local sorted
    sorted=$(echo "$filtered_rand" | sort_by_layer)
    while IFS= read -r entry; do
      [[ -z "$entry" ]] && continue
      local layer="${entry:0:2}" content="${entry:3}"
      [[ "$layer" == "99" ]] && layer="?"
      layer="${layer#0}"
      layer_filter_matches "$layer" || continue
      violation "[L${layer}] Direct randomness: $content"
      hint "Use RandomEffects trait"
    done <<< "$sorted"
  else
    info "Direct randomness: none"
  fi
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Guard Chain
# ═══════════════════════════════════════════════════════════════════════════════
check_guards() {
  section "Guard chain — TransportEffects must flow through CapGuard → FlowGuard → JournalCoupler"

  local transport_sends bypass_hits
  transport_sends=$(rg --no-heading "TransportEffects::(send|open_channel)" crates -g "*.rs" || true)
  local guard_allow="crates/aura-guards/src/guards|crates/aura-protocol/src/handlers/sessions|crates/aura-agent/src/runtime/effects.rs|crates/aura-agent/src/runtime/effects/choreography.rs|tests/|crates/aura-testkit/"
  bypass_hits=$(echo "$transport_sends" | grep -Ev "$guard_allow" || true)
  emit_hits "Potential guard-chain bypass" "$bypass_hits"
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Invariant Documentation
# ═══════════════════════════════════════════════════════════════════════════════
check_invariants() {
  section "Invariant docs — INVARIANTS.md must include required headings"

  local inv_files
  inv_files=$(find crates -name INVARIANTS.md 2>/dev/null | sort)
  [[ -z "$inv_files" ]] && { violation "No INVARIANTS.md files found"; return; }

  for inv in $inv_files; do
    local missing=()
    for heading in "Invariant Name" "Enforcement Locus" "Failure Mode" "Detection Method"; do
      grep -q "$heading" "$inv" || missing+=("$heading")
    done
    [[ ${#missing[@]} -gt 0 ]] && violation "Missing sections [$(IFS=,; echo "${missing[*]}")]: $inv" || info "OK: $inv"
  done
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Handler Registration
# ═══════════════════════════════════════════════════════════════════════════════
check_registration() {
  section "Handler composition — instantiate via EffectRegistry, not direct new()"

  local handler_pattern="(aura_effects::.*Handler::new|PhysicalTimeHandler::new|RealRandomHandler::new|FilesystemStorageHandler::new|EncryptedStorageHandler::new|TcpNetworkHandler::new|RealCryptoHandler::new)"
  local instantiation
  instantiation=$(rg --no-heading "$handler_pattern" crates/aura-protocol/src crates/aura-authentication/src crates/aura-chat/src crates/aura-invitation/src crates/aura-recovery/src crates/aura-relational/src crates/aura-rendezvous/src crates/aura-sync/src -g "*.rs" -g "!tests/**/*" || true)
  emit_hits "Direct handler instantiation" "$instantiation"
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Crypto Boundaries
# ═══════════════════════════════════════════════════════════════════════════════
check_crypto() {
  section "Crypto boundaries — route through aura-core wrappers"

  # ed25519_dalek imports
  local ed_hits filtered_ed
  ed_hits=$(rg --no-heading "use ed25519_dalek" crates -g "*.rs" || true)
  filtered_ed=$(echo "$ed_hits" | grep -Ev "$ALLOW_CRYPTO" || true)
  if [[ -n "$filtered_ed" ]]; then
    emit_hits "Direct ed25519_dalek import" "$filtered_ed"
    verbose "Allowed: aura-core/crypto, aura-effects, aura-testkit, tests"
  else
    info "ed25519_dalek: only in allowed locations"
  fi

  # OsRng usage
  local osrng_hits filtered_osrng
  osrng_hits=$(rg --no-heading "OsRng" crates -g "*.rs" | grep -v "///" | grep -v "//!" | grep -v "// " || true)
  filtered_osrng=$(echo "$osrng_hits" | grep -Ev "$ALLOW_RANDOM" || true)
  filtered_osrng=$(filter_test_modules "$filtered_osrng")
  if [[ -n "$filtered_osrng" ]]; then
    emit_hits "Direct OsRng usage" "$filtered_osrng"
  else
    info "OsRng: only in allowed locations"
  fi

  # getrandom usage
  local getrandom_hits filtered_getrandom
  getrandom_hits=$(rg --no-heading "getrandom::" crates -g "*.rs" | grep -v "///" | grep -v "//" || true)
  filtered_getrandom=$(echo "$getrandom_hits" | grep -Ev "$ALLOW_RANDOM" || true)
  if [[ -n "$filtered_getrandom" ]]; then
    emit_hits "Direct getrandom usage" "$filtered_getrandom"
  else
    info "getrandom: only in allowed locations"
  fi
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Concurrency Hygiene
# ═══════════════════════════════════════════════════════════════════════════════
check_concurrency() {
  section "Concurrency — avoid block_in_place and unbounded channels"

  local block_hits filtered_block
  block_hits=$(rg --no-heading "tokio::task::block_in_place|Handle::current\\(\\)\\.block_on" crates -g "*.rs" || true)
  filtered_block=$(filter_allow "$block_hits")
  emit_hits "Blocking async bridge" "$filtered_block"

  local unbounded_hits filtered_unbounded
  unbounded_hits=$(rg --no-heading "mpsc::unbounded_channel\\(|async_channel::unbounded\\(|mpsc::unbounded\\(" crates -g "*.rs" || true)
  filtered_unbounded=$(filter_allow "$unbounded_hits")
  emit_hits "Unbounded channel" "$filtered_unbounded"
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

  # Journal/protocol mutation
  local journal_hits
  journal_hits=$(rg --no-heading "FactRegistry|FactReducer|RelationalFact|JournalEffects|commit_.*facts|RuntimeBridge::commit" crates/aura-terminal/src -g "*.rs" || true)
  emit_hits "Direct journal/protocol mutation" "$journal_hits"

  # Forbidden crate usage
  local forbidden
  forbidden=$(rg --no-heading "aura_(journal|protocol|consensus|guards|amp|anti_entropy|transport|recovery|sync|invitation|authentication|relational|chat)::" crates/aura-terminal/src -g "*.rs" || true)
  emit_hits "Direct protocol/domain crate usage" "$forbidden"

  # ─── Terminal time ───
  section "Terminal time — use algebraic effects"
  local time_hits
  time_hits=$(rg --no-heading -n "SystemTime::now|Instant::now|std::time::Instant|std::time::SystemTime|chrono::Utc::now|chrono::Local::now" crates/aura-terminal/src -g "*.rs" \
    | grep -v "///" | grep -v "//!" | grep -v "//" || true)
  emit_hits "Direct OS time in terminal" "$time_hits"

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
    | grep -v "crates/aura-app/src/workflows/runtime.rs" | grep -v '\.contains(' || true)
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
    | grep -v "crates/aura-app/src/core/app.rs" | grep -v "init_signals_with_hooks" || true)
  emit_hits "Direct init_signals calls" "$init_calls"
}


# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Serialization
# ═══════════════════════════════════════════════════════════════════════════════
check_serialization() {
  section "Serialization — use DAG-CBOR; no bincode"

  # bincode usage
  local bincode_hits
  bincode_hits=$(rg --no-heading "bincode::" crates -g "*.rs" | grep -Ev "/examples/|benches/" || true)
  if [[ -n "$bincode_hits" ]]; then
    emit_hits "bincode usage" "$bincode_hits"
    hint "Migrate: bincode::serialize → to_vec, deserialize → from_slice"
  else
    info "bincode: none"
  fi

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

  # SessionId::new outside tests
  section "Identifier invariants — SessionId::new() is test-only"
  local session_hits
  session_hits=$(rg --no-heading "SessionId::new\\(" crates -g "*.rs" \
    | grep -Ev "/tests/|/benches/|/examples/|cfg(test)|cfg\\(test\\)" || true)
  emit_hits "SessionId::new() outside tests" "$session_hits"

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

  local handler_state
  handler_state=$(rg --no-heading "Arc<.*(RwLock|Mutex)|RwLock<|Mutex<" crates/aura-agent/src/handlers -g "*.rs" || true)
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

  # usize in serialized structs
  local usize_hits field_usize
  usize_hits=$(rg --no-heading -n "usize" crates -g "*.rs" \
    | xargs -I{} sh -c 'file="${1%%:*}"; grep -l "#\[derive.*Serialize" "$file" >/dev/null 2>&1 && echo "$1"' _ {} 2>/dev/null || true)
  field_usize=$(echo "$usize_hits" \
    | grep -Ev "/tests/|/benches/|crates/aura-testkit/" \
    | grep -v "// usize ok:" \
    | grep -Ev "fn |let |for |impl " \
    | grep -E ":\s*usize\s*[,}]|pub\s+\w+:\s*usize" || true)
  emit_hits "usize in serialized field (use u32/u64)" "$field_usize"

  # Vec<u8> without MAX_* bounds
  local unbounded_bytes missing_bounds=""
  unbounded_bytes=$(rg --no-heading -n "pub\s+\w+:\s*Vec<u8>" crates/aura-core/src -g "*.rs" || true)
  while IFS= read -r hit; do
    [[ -z "$hit" ]] && continue
    local file="${hit%%:*}"
    grep -qE "const\s+MAX_.*_(BYTES|SIZE|LEN)" "$file" 2>/dev/null || missing_bounds+="$hit"$'\n'
  done <<< "$unbounded_bytes"
  emit_hits "Vec<u8> without MAX_* constant" "$missing_bounds"

  # Numeric constants without unit suffixes
  local constants_no_units
  constants_no_units=$(rg --no-heading -n "const\s+[A-Z][A-Z0-9_]+:\s*(u\d+|i\d+|usize)\s*=\s*\d+" crates/aura-core/src -g "*.rs" \
    | grep -vE "_(MS|BYTES|COUNT|SIZE|MAX|MIN|LEN|LIMIT|DEPTH|HEIGHT|BITS|SECS|NANOS)(\s*:|:)" \
    | grep -vE "VERSION|MAGIC|EPOCH|THRESHOLD|FACTOR|RATIO|WIRE_FORMAT|DEFAULT_" \
    | grep -Ev "/tests/|/benches/" || true)
  [[ -n "$constants_no_units" ]] && emit_hits "Constant without unit suffix" "$constants_no_units" || info "Constants: all have units"

  # Builder methods without #[must_use]
  local builder_methods missing_must_use=""
  builder_methods=$(rg --no-heading -n "pub\s+(const\s+)?fn\s+with_\w+\s*\(" crates/aura-core/src -g "*.rs" || true)
  if [[ -n "$builder_methods" ]]; then
    while IFS= read -r hit; do
      [[ -z "$hit" ]] && continue
      local file rest linenum
      file="${hit%%:*}"
      rest="${hit#*:}"
      linenum="${rest%%:*}"
      local has=false
      for offset in 1 2 3; do
        local prev=$((linenum - offset))
        [[ "$prev" -gt 0 ]] && sed -n "${prev}p" "$file" 2>/dev/null | grep -qE "#\[must_use" && { has=true; break; }
      done
      $has || missing_must_use+="$hit"$'\n'
    done <<< "$builder_methods"
  fi
  emit_hits "Builder without #[must_use]" "$missing_must_use"

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
# CHECK: TODOs and Incomplete Markers
# ═══════════════════════════════════════════════════════════════════════════════
check_todos() {
  # Placeholders
  section "Placeholders — replace nil UUIDs with real derivations"
  local placeholder_hits
  placeholder_hits=$(rg --no-heading -i "uuid::nil\\(\\)|placeholder implementation" crates -g "*.rs" \
    | grep -Ev "/tests/|/benches/|/examples/" || true)
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
  local todo_hits
  todo_hits=$(rg --no-heading "TODO|FIXME" crates -g "*.rs" \
    | grep -Ev "/tests/|/benches/|/examples/" \
    | grep -Ev "$platform_allow" \
    | grep -Ev "$tui_allow" || true)
  emit_hits "TODO/FIXME" "$todo_hits"

  # Incomplete markers
  section "Incomplete/WIP markers"
  local incomplete_pattern="in production[^\\n]*(would|should|not)|stub|not implemented|unimplemented|temporary|workaround|hacky|\\bWIP\\b|\\bTBD\\b|prototype|future work|to be implemented"
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
# ═══════════════════════════════════════════════════════════════════════════════
{ $RUN_ALL || $RUN_LAYERS; }       && check_layers
{ $RUN_ALL || $RUN_DEPS; }         && check_deps
{ $RUN_ALL || $RUN_EFFECTS; }      && check_effects
{ $RUN_ALL || $RUN_GUARDS; }       && check_guards
{ $RUN_ALL || $RUN_INVARIANTS; }   && check_invariants
{ $RUN_ALL || $RUN_REG; }          && check_registration
{ $RUN_ALL || $RUN_CRYPTO; }       && check_crypto
{ $RUN_ALL || $RUN_CONCURRENCY; }  && check_concurrency
{ $RUN_ALL || $RUN_REACTIVE; }     && check_reactive
{ $RUN_ALL || $RUN_UI; }           && check_ui
{ $RUN_ALL || $RUN_WORKFLOWS; }    && check_workflows
{ $RUN_ALL || $RUN_SERIALIZATION; } && { check_serialization; check_handler_hygiene; }
{ $RUN_ALL || $RUN_STYLE; }        && check_style
{ $RUN_ALL || $RUN_TODOS; }        && check_todos


# ═══════════════════════════════════════════════════════════════════════════════
# Summary
# ═══════════════════════════════════════════════════════════════════════════════
echo -e "\n${BOLD}${CYAN}Summary${NC}"
if [[ $VIOLATIONS -eq 0 ]]; then
  echo -e "${GREEN}✔ No violations${NC}"
else
  echo -e "${RED}✖ $VIOLATIONS violation(s)${NC}"
  if $VERBOSE && [[ ${#VIOLATION_DETAILS[@]} -gt 0 ]]; then
    echo -e "\n${BOLD}Violation details:${NC}"
    for d in "${VIOLATION_DETAILS[@]}"; do echo "  - $d"; done
  fi
fi

[[ $VIOLATIONS -gt 10 ]] && ! $RUN_QUICK && echo -e "\n${YELLOW}Tip:${NC} Use --quick to skip TODO/placeholder checks"

exit $([[ $VIOLATIONS -eq 0 ]] && echo 0 || echo 1)
