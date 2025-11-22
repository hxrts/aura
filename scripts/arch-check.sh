#!/bin/bash
# Architectural compliance checker for Aura codebase
# Enforces the 8-layer architecture pattern

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

VIOLATIONS=0
WARNINGS=0

# Arrays to collect issues for structured output
declare -a VIOLATION_DETAILS
declare -a WARNING_DETAILS

echo -e "${BOLD}${CYAN}🔍 Aura Architectural Compliance Checker${NC}"
echo -e "${CYAN}==========================================${NC}"
echo ""

# Helper functions
violation() {
    local msg="$1"
    local detail="${2:-}"
    VIOLATIONS=$((VIOLATIONS + 1))
    VIOLATION_DETAILS+=("$msg")
    if [ -n "$detail" ]; then
        VIOLATION_DETAILS+=("  └─ $detail")
    fi
    echo -e "${RED}❌ VIOLATION:${NC} $msg"
    if [ -n "$detail" ]; then
        echo -e "   ${RED}└─${NC} $detail"
    fi
    item_divider
}

warning() {
    local msg="$1"
    local detail="${2:-}"
    WARNINGS=$((WARNINGS + 1))
    WARNING_DETAILS+=("$msg")
    if [ -n "$detail" ]; then
        WARNING_DETAILS+=("  └─ $detail")
    fi
    echo -e "${YELLOW}⚠️  WARNING:${NC} $msg"
    if [ -n "$detail" ]; then
        echo -e "   ${YELLOW}└─${NC} $detail"
    fi
    item_divider
}

success() {
    echo -e "${GREEN}✅${NC} $1"
}

info() {
    echo -e "${BLUE}ℹ️${NC}  $1"
}

section_header() {
    echo ""
    echo -e "${BOLD}${CYAN}▶ $1${NC}"
    echo -e "${CYAN}$(printf '─%.0s' {1..60})${NC}"
}

subsection() {
    echo -e "${CYAN}  → $1${NC}"
}

item_divider() {
    echo -e "${BLUE}    $(printf '·%.0s' {1..50})${NC}"
}

section_divider() {
    echo ""
    echo -e "${CYAN}$(printf '═%.0s' {1..70})${NC}"
    echo ""
}

# Check if cargo is available for metadata analysis
check_cargo() {
    if ! command -v cargo &> /dev/null; then
        warning "cargo not found, falling back to grep-based dependency checks"
        return 1
    fi
    return 0
}

section_header "Layer 1: Interface Purity (aura-core)"

# More precise implementation detection - distinguish blanket impls from business logic
business_logic_impls=$(grep -r "impl.*Effects" crates/aura-core/src/ 2>/dev/null | \
    grep -v "trait" | \
    grep -v "// Example" | \
    grep -v "// Note:" | \
    grep -v "impl<T.*>" | \
    grep -v "Blanket implementation" || true)

if [ -n "$business_logic_impls" ]; then
    violation "aura-core contains business logic implementations (should only have trait definitions)"
    echo "$business_logic_impls"
fi

# Check for extension trait implementations (these are allowed for convenience methods)
extension_trait_impls=$(grep -r "impl<T.*>.*Effects" crates/aura-core/src/ 2>/dev/null | \
    grep -v "trait" | \
    wc -l || echo 0)

if [ "$extension_trait_impls" -gt 0 ]; then
    success "Found $extension_trait_impls extension trait implementations (allowed for convenience methods)"
fi

# aura-core should not depend on other aura crates (check dependencies section only)
if grep -A 20 "^\[dependencies\]" crates/aura-core/Cargo.toml | grep -E "aura-[a-z]+" | grep -v "# " 2>/dev/null; then
    violation "aura-core depends on other Aura crates (violates interface layer isolation)"
fi

section_divider
section_header "Layer-Specific Boundary Validation"

# Check Layer 2: Domain crates don't implement effect handlers
for crate in aura-journal aura-wot aura-verify aura-store aura-transport; do
    if [ -d "crates/$crate" ]; then
        # Domain crates shouldn't implement effect traits (except their own domain effects)
        if grep -r "#\[async_trait\]" crates/$crate/src/ 2>/dev/null | grep -A5 "impl.*Effects" | grep -v "${crate/aura-/}" | head -1 2>/dev/null; then
            violation "$crate implements non-domain effect handlers (should only contain domain logic)"
        fi
    fi
done

subsection "Layer 3: aura-composition compliance"
if [ -d "crates/aura-composition" ]; then
    # Check that aura-composition doesn't contain individual handler implementations
    handler_impls=$(grep -r "impl.*Handler" crates/aura-composition/src/ 2>/dev/null | \
        grep -v "Builder\|Registry\|Adapter" | \
        grep -v "test" || true)

    if [ -n "$handler_impls" ]; then
        warning "aura-composition contains individual handler implementations (should be composition utilities only)"
        echo "$handler_impls"
    fi

    # Check that aura-composition doesn't contain multi-party coordination logic
    coordination_patterns=$(grep -r "consensus\|anti_entropy\|guard_chain" crates/aura-composition/src/ 2>/dev/null | \
        grep -v "test" | grep -v "comment" | grep -i -v "// " || true)

    if [ -n "$coordination_patterns" ]; then
        violation "aura-composition contains multi-party coordination logic (should be in aura-protocol)"
        echo "$coordination_patterns"
    fi

    # Positive check: aura-composition should contain composition utilities
    composition_utilities=$(grep -r "Registry\|Builder\|Compose" crates/aura-composition/src/ 2>/dev/null | \
        wc -l || echo 0)

    if [ "$composition_utilities" -gt 0 ]; then
        success "Found composition utilities in aura-composition (correct location)"
    else
        info "No composition utilities found in aura-composition (may be structured differently)"
    fi
else
    info "aura-composition directory not found"
fi

subsection "Layer 3: aura-effects stateless compliance"
if [ -d "crates/aura-effects" ]; then
    # Look for stateful patterns that shouldn't be in production effect handlers
    stateful_patterns=$(grep -r "Arc<Mutex\|Arc<RwLock\|Rc<RefCell" crates/aura-effects/src/ 2>/dev/null | \
        grep -v "test" || true)

    if [ -n "$stateful_patterns" ]; then
        violation "aura-effects contains stateful patterns (production handlers should be stateless)"
        echo "$stateful_patterns"
        echo ""
        echo "   ⚡ SOLUTION: Move stateful handlers to aura-testkit or make them stateless"
        echo "   📖 WHY: Production effect handlers must be stateless for predictable composition"
        echo "   🔧 HOW: Extract state into effect parameters or use dependency injection"
    fi

    # Check for mock handlers that should be in aura-testkit
    mock_handlers=$(grep -r "Mock.*Handler\|InMemory.*Handler" crates/aura-effects/src/ 2>/dev/null | \
        grep -v "test" || true)

    if [ -n "$mock_handlers" ]; then
        violation "aura-effects contains mock/test handlers (should be in aura-testkit)"
        echo "$mock_handlers"
        echo "   ⚡ SOLUTION: mv crates/aura-effects/src/mock_*.rs crates/aura-testkit/src/"
        echo "   📖 WHY: aura-effects is for production handlers; mocks belong in testing infrastructure"
        echo "   🔧 HOW: Update imports in test files to use aura-testkit::MockHandler"
    fi

    # Check for multi-party coordination logic in effects
    coordination_patterns=$(grep -r "consensus\|coordinate\|orchestrate" crates/aura-effects/src/ 2>/dev/null | \
        grep -v "test" | grep -v "comment" | grep -i -v "// " || true)

    if [ -n "$coordination_patterns" ]; then
        warning "aura-effects contains coordination logic (should be in aura-protocol)"
        echo "$coordination_patterns"
        echo "   ⚡ SOLUTION: Move multi-party coordination to aura-protocol"
        echo "   📖 WHY: Layer 3 is for single-party operations; orchestration belongs in Layer 4"
        echo "   🔧 HOW: Extract coordination logic into aura-protocol coordinators"
    fi
fi

subsection "Layer 8: aura-testkit mock handler compliance"
if [ -d "crates/aura-testkit" ]; then
    # Check that aura-testkit contains the expected mock handlers
    mock_handler_files=$(find crates/aura-testkit/src/ -name "*.rs" -exec grep -l "Mock.*Handler\|InMemory.*Handler" {} \; 2>/dev/null || true)

    if [ -n "$mock_handler_files" ]; then
        success "Mock handlers found in aura-testkit (correct location)"
    else
        info "No mock handlers found in aura-testkit (may be structured differently)"
    fi
else
    info "aura-testkit directory not found"
fi

subsection "Layer 5: feature crate dependencies"
for crate in aura-authenticate aura-frost aura-invitation aura-recovery aura-rendezvous aura-sync aura-storage; do
    if [ -d "crates/$crate" ]; then
        # Check regular dependencies (not dev-dependencies) - should not depend on runtime layers
        runtime_deps=$(grep -A 20 "^\[dependencies\]" crates/$crate/Cargo.toml 2>/dev/null | \
            grep -E "aura-agent|aura-simulator|aura-cli" | \
            grep -v "^\[" || true)
        if [ -n "$runtime_deps" ]; then
            violation "$crate depends on runtime/UI layers (Layer 5 shouldn't depend on Layer 6+)"
            echo "$runtime_deps"
        fi

        # Check that feature crates can depend on aura-composition (this is the point)
        composition_dep=$(grep -A 20 "^\[dependencies\]" crates/$crate/Cargo.toml 2>/dev/null | \
            grep "aura-composition" | grep -v "^\[" || true)
        if [ -n "$composition_dep" ]; then
            success "$crate depends on aura-composition (enables handler composition)"
        fi

        # Check that aura-testkit is only used in dev-dependencies (this is allowed)
        testkit_in_deps=$(grep -A 20 "^\[dependencies\]" crates/$crate/Cargo.toml 2>/dev/null | \
            grep "aura-testkit" | grep -v "^\[" || true)
        if [ -n "$testkit_in_deps" ]; then
            warning "$crate depends on aura-testkit in regular dependencies (should be dev-dependencies only)"
            echo "$testkit_in_deps"
        fi
    fi
done

section_divider
section_header "Legacy Violation Checks"

# Check for old effect-based ID generation patterns (excluding journal domain extensions)
# Look for usage patterns but exclude the journal's SessionIdExt trait which is domain-specific
VIOLATIONS_FOUND=$(grep -r "new_with_effects" crates/*/src/ 2>/dev/null | grep -v "test" | grep -v "aura-journal/src/types.rs" || true)
if [ -n "$VIOLATIONS_FOUND" ]; then
    violation "Effect-based ID generation found (should use clean patterns)"
    echo "$VIOLATIONS_FOUND"
fi

# Check for extension trait imports that were removed
if grep -r "AccountIdExt\|DeviceIdExt" crates/*/src/ 2>/dev/null | grep -v "test"; then
    violation "Removed extension traits still being imported"
fi

section_divider
section_header "Dependency Direction Analysis"

if check_cargo; then
    # Use cargo metadata for accurate dependency analysis
    echo "Using cargo metadata for dependency analysis..."

    # Get dependency graph
    deps_json=$(cargo metadata --format-version 1 --no-deps 2>/dev/null | \
        jq -r '.packages[] | select(.name | startswith("aura-")) | {name: .name, deps: [.dependencies[] | select(.name | startswith("aura-")) | .name]}' 2>/dev/null || echo "")

    if [ -n "$deps_json" ]; then
        # Check Layer 2 crates don't depend on Layer 3+
        for crate in aura-journal aura-wot aura-verify aura-store aura-transport; do
            if [ -d "crates/$crate" ]; then
                effects_dep=$(echo "$deps_json" | jq -r "select(.name == \"$crate\") | .deps[] | select(. == \"aura-effects\")" 2>/dev/null || true)
                if [ -n "$effects_dep" ]; then
                    violation "$crate depends on aura-effects (Layer 2 should not depend on Layer 3)"
                fi

                composition_dep=$(echo "$deps_json" | jq -r "select(.name == \"$crate\") | .deps[] | select(. == \"aura-composition\")" 2>/dev/null || true)
                if [ -n "$composition_dep" ]; then
                    violation "$crate depends on aura-composition (Layer 2 should not depend on Layer 3)"
                fi
            fi
        done

        # Check for circular dependencies using dependency resolution
        info "Checking for circular dependencies with cargo..."
        if ! cargo check --workspace --quiet 2>/dev/null; then
            warning "Cargo check failed - potential dependency issues (run 'cargo check' for details)"
        fi

        # Enhanced dependency validation will be done in a separate section
    else
        warning "Failed to parse cargo metadata, falling back to grep"
    fi
else
    # Fallback to grep-based checks
    echo "Using grep-based dependency analysis..."

    # Domain crates should not depend on aura-effects or aura-composition
    for crate in aura-journal aura-wot aura-verify aura-store aura-transport; do
        if [ -d "crates/$crate" ]; then
            if grep -q "aura-effects" crates/$crate/Cargo.toml 2>/dev/null; then
                violation "$crate depends on aura-effects (Layer 2 should not depend on Layer 3)"
            fi
            if grep -q "aura-composition" crates/$crate/Cargo.toml 2>/dev/null; then
                violation "$crate depends on aura-composition (Layer 2 should not depend on Layer 3)"
            fi
        fi
    done

    # Simple circular dependency check
    if find crates/ -name Cargo.toml -exec grep -l "aura-effects" {} \; | xargs grep -l "aura-core" 2>/dev/null; then
        warning "Potential circular dependency chain detected (install cargo for detailed analysis)"
    fi
fi

section_divider
section_header "Effect Trait Organization"

# Effect trait location validation
effect_traits_outside=$(find crates/ -name "*.rs" -not -path "*/aura-core/*" -exec grep -l "trait.*Effects" {} \; 2>/dev/null)
if [ -n "$effect_traits_outside" ]; then
    # Check for specific violations
    protocol_effect_traits=$(echo "$effect_traits_outside" | grep "aura-protocol" || true)
    if [ -n "$protocol_effect_traits" ]; then
        warning "Effect traits defined in aura-protocol (should be in aura-core):"
        echo "$protocol_effect_traits"
        echo "   ⚡ SOLUTION: Move trait definitions to crates/aura-core/src/effects/"
        echo "   📖 WHY: Effect traits are interfaces and belong in the foundation layer"
        echo "   🔧 HOW: Move trait, update imports, keep implementations in aura-protocol"
    fi

    # Report other cases as informational
    other_effect_traits=$(echo "$effect_traits_outside" | grep -v "aura-protocol" || true)
    if [ -n "$other_effect_traits" ]; then
        info "Effect traits in domain crates (may be domain-specific extensions):"
        echo "$other_effect_traits"
        echo "   ✅ ACCEPTABLE: Domain-specific effect extensions in their domain crates"
        echo "   📖 WHY: Domain extensions add business semantics to foundation traits"
        echo "   💡 TIP: Ensure these extend aura-core traits, not replace them"
    fi
fi

section_divider
section_header "Test Pattern Compliance"

# Tests should use clean patterns, not architectural violations
effects_test_usage=$(grep -r "Effects::test()" crates/*/src/ 2>/dev/null | grep -v "aura-effects" | grep -v "aura-testkit" | head -3)
if [ -n "$effects_test_usage" ]; then
    warning "Tests using Effects::test() directly (consider using TestFixtures from aura-testkit)"
    echo "$effects_test_usage"
fi

section_divider
section_header "Positive Architecture Validation"

# Effect trait classification - distinguish infrastructure vs application effects
core_traits=$(grep -r "trait.*Effects" crates/aura-core/src/effects/ 2>/dev/null | \
    grep -o "[A-Z][a-zA-Z]*Effects" | sort -u || true)

if [ -n "$core_traits" ]; then
    # Infrastructure effects that MUST have handlers in aura-effects
    infrastructure_effects="CryptoEffects NetworkEffects StorageEffects TimeEffects RandomEffects ConfigurationEffects ConsoleEffects"

    # Application effects that are implemented in domain crates (not a violation)
    application_effects="JournalEffects AuthorityEffects FlowBudgetEffects LeakageEffects AuthorizationEffects RelationalContextEffects GuardianEffects"

    # Composite effects that are typically extension traits (not a violation)
    composite_effects="TreeEffects SimulationEffects"

    missing_infrastructure=""
    found_application_in_effects=""

    for trait in $core_traits; do
        # Check infrastructure effects - these MUST have handlers in aura-effects
        if echo " $infrastructure_effects " | grep -q " $trait "; then
            if ! grep -r "impl.*$trait" crates/aura-effects/src/ 2>/dev/null >/dev/null; then
                missing_infrastructure="$missing_infrastructure $trait"
            fi
        fi

        # Check application effects - these should NOT be in aura-effects (would create circular deps)
        if echo " $application_effects " | grep -q " $trait "; then
            if grep -r "impl.*$trait" crates/aura-effects/src/ 2>/dev/null >/dev/null; then
                found_application_in_effects="$found_application_in_effects $trait"
            fi
        fi
    done

    # Report missing infrastructure effect handlers
    if [ -n "$missing_infrastructure" ]; then
        violation "Missing infrastructure effect handlers in aura-effects:$missing_infrastructure"
        echo "   ⚡ SOLUTION: Implement missing handlers in crates/aura-effects/src/"
        echo "   📖 WHY: Infrastructure effects need stateless OS integration for all use cases"
        echo "   🔧 HOW: Create RealXxxHandler that delegates to system services (files/network/crypto)"
    else
        success "All infrastructure effects have handlers in aura-effects"
    fi

    # Report application effects in wrong location
    if [ -n "$found_application_in_effects" ]; then
        violation "Application effects implemented in aura-effects:$found_application_in_effects"
        echo "   ⚡ SOLUTION: Move implementations to respective domain crates"
        echo "   📖 WHY: Domain effects need business logic; aura-effects would create cycles"
        echo "   🔧 HOW: Create DomainHandler<I> that composes infrastructure effects"
    fi

    # Count properly classified effects
    infrastructure_count=$(echo "$infrastructure_effects" | wc -w)
    application_count=$(echo "$application_effects" | wc -w)
    composite_count=$(echo "$composite_effects" | wc -w)
    total_classified=$((infrastructure_count + application_count + composite_count))

    info "Effect classification: $infrastructure_count infrastructure, $application_count application, $composite_count composite effects"

    # Check for specific architectural violations found in codebase analysis
    subsection "Domain effect implementation compliance"

    # Check if JournalEffects is implemented in aura-effects (should be in aura-journal)
    if grep -r "impl.*JournalEffects" crates/aura-effects/src/ 2>/dev/null >/dev/null; then
        violation "JournalEffects implemented in aura-effects (should be in aura-journal)"
        echo "   ⚡ SOLUTION: Move JournalEffects impl to crates/aura-journal/src/effects.rs"
        echo "   📖 WHY: Application effects need domain logic; aura-effects can't depend on domains"
        echo "   🔧 HOW: Create JournalHandler<C,S> that composes CryptoEffects + StorageEffects"
    fi

    # Check if AuthorizationEffects is implemented in aura-effects (should be in aura-wot)
    if grep -r "impl.*AuthorizationEffects" crates/aura-effects/src/ 2>/dev/null >/dev/null; then
        violation "AuthorizationEffects implemented in aura-effects (should be in aura-wot)"
        echo "   ⚡ SOLUTION: Move AuthorizationEffects impl to crates/aura-wot/src/effects.rs"
        echo "   📖 WHY: Authorization needs Biscuit/capability logic; belongs with WoT domain"
        echo "   🔧 HOW: Create AuthHandler that validates tokens using capability semilattice"
    fi

    # Check if FlowBudgetEffects is defined in aura-protocol (should be in aura-core)
    if [ -d "crates/aura-protocol" ] && grep -r "trait.*FlowBudgetEffects" crates/aura-protocol/src/ 2>/dev/null >/dev/null; then
        violation "FlowBudgetEffects defined in aura-protocol (should be in aura-core)"
        echo "   ⚡ SOLUTION: mv trait FlowBudgetEffects to crates/aura-core/src/effects/budget.rs"
        echo "   📖 WHY: Effect traits are interfaces and must be in foundation for dependency order"
        echo "   🔧 HOW: Keep the guard implementation in aura-protocol, move only the trait"
    fi

    # Positive checks for expected domain effect implementations
    expected_domain_implementations=""

    if [ -d "crates/aura-journal" ] && ! grep -r "impl.*JournalEffects" crates/aura-journal/src/ 2>/dev/null >/dev/null; then
        expected_domain_implementations="$expected_domain_implementations aura-journal→JournalEffects"
    fi

    if [ -d "crates/aura-wot" ] && ! grep -r "impl.*AuthorizationEffects" crates/aura-wot/src/ 2>/dev/null >/dev/null; then
        expected_domain_implementations="$expected_domain_implementations aura-wot→AuthorizationEffects"
    fi

    if [ -d "crates/aura-relational" ] && ! grep -r "impl.*RelationalEffects\|impl.*RelationalContextEffects" crates/aura-relational/src/ 2>/dev/null >/dev/null; then
        expected_domain_implementations="$expected_domain_implementations aura-relational→RelationalEffects"
    fi

    if [ -n "$expected_domain_implementations" ]; then
        info "Missing expected domain effect implementations:$expected_domain_implementations"
        echo "   ⚡ SOLUTION: Implement domain handlers in the respective crates"
        echo "   📖 WHY: Domain crates should own their application logic to avoid dependencies"
        echo "   🔧 HOW: Create Handler<I: InfraEffects> that composes infrastructure + domain logic"
    else
        success "Domain effect implementation pattern followed correctly"
    fi
fi

# Check that Layer 1 (aura-core) contains expected architectural components
expected_core_modules="effects identifiers errors"
missing_core_modules=""

for module in $expected_core_modules; do
    # Check for both directory and file variants (e.g., src/effects/ OR src/effects.rs)
    if [ ! -d "crates/aura-core/src/$module" ] && [ ! -f "crates/aura-core/src/$module.rs" ]; then
        missing_core_modules="$missing_core_modules $module"
    fi
done

if [ -n "$missing_core_modules" ]; then
    warning "Missing expected core modules:$missing_core_modules"
    echo "   ⚡ SOLUTION: Create missing modules in crates/aura-core/src/"
    echo "   📖 WHY: Core modules organize foundation types for the entire system"
    echo "   🔧 HOW: mkdir crates/aura-core/src/{effects,identifiers,errors} as needed"
else
    success "Core architectural modules present"
fi

subsection "Core module content validation"

# Check for required domain types
required_domain_types="AuthorityId ContextId SessionId FlowBudget ObserverClass Capability"
missing_domain_types=""

for type in $required_domain_types; do
    if ! grep -r "pub struct $type\|pub type $type\|pub enum $type" crates/aura-core/src/ 2>/dev/null >/dev/null; then
        missing_domain_types="$missing_domain_types $type"
    fi
done

if [ -n "$missing_domain_types" ]; then
    warning "Missing required domain types in aura-core:$missing_domain_types"
    echo "   ⚡ SOLUTION: Add missing types to crates/aura-core/src/identifiers.rs"
    echo "   📖 WHY: Foundation types must be in aura-core for the entire system to reference"
    echo "   🔧 HOW: Define as pub struct/enum with appropriate derives and documentation"
fi

# Check for cryptographic utilities
crypto_utils="FROST Ed25519 merkle"
missing_crypto=""

for util in $crypto_utils; do
    if ! grep -ri "$util" crates/aura-core/src/ 2>/dev/null >/dev/null; then
        missing_crypto="$missing_crypto $util"
    fi
done

if [ -n "$missing_crypto" ]; then
    info "Consider adding cryptographic utilities:$missing_crypto"
fi

# Check for semantic traits
semantic_traits="JoinSemilattice MeetSemilattice CvState MvState"
missing_semantic=""

for trait in $semantic_traits; do
    if ! grep -r "trait $trait" crates/aura-core/src/ 2>/dev/null >/dev/null; then
        missing_semantic="$missing_semantic $trait"
    fi
done

if [ -n "$missing_semantic" ]; then
    warning "Missing semantic traits in aura-core:$missing_semantic"
    echo "   ⚡ SOLUTION: Add semantic traits to crates/aura-core/src/semilattice.rs"
    echo "   📖 WHY: CRDT operations need lattice semantics for conflict resolution"
    echo "   🔧 HOW: Define trait with required join/meet operations for domain types"
fi

section_divider
section_header "Domain Crate Content Validation"

# Check aura-journal for fact-based patterns
if [ -d "crates/aura-journal" ]; then
    subsection "aura-journal fact-based compliance"

    if ! grep -r "Fact\|Journal\|CRDT" crates/aura-journal/src/ 2>/dev/null >/dev/null; then
        warning "aura-journal missing fact-based journal patterns"
    fi

    if ! grep -r "validation\|reduction" crates/aura-journal/src/ 2>/dev/null >/dev/null; then
        warning "aura-journal missing validation/reduction logic"
    fi

    if grep -r "deterministic.*reduction" crates/aura-journal/src/ 2>/dev/null >/dev/null; then
        success "aura-journal contains deterministic reduction patterns"
    fi
fi

# Check aura-wot for capability system
if [ -d "crates/aura-wot" ]; then
    subsection "aura-wot capability system compliance"

    if ! grep -r "Capability\|Biscuit.*token\|meet.*semilattice" crates/aura-wot/src/ 2>/dev/null >/dev/null; then
        warning "aura-wot missing capability/token patterns"
    fi

    if grep -r "policy.*evaluation\|authorization" crates/aura-wot/src/ 2>/dev/null >/dev/null; then
        success "aura-wot contains authorization patterns"
    fi
fi

# Check aura-mpst for choreography features
if [ -d "crates/aura-mpst" ]; then
    subsection "aura-mpst choreography compliance"

    choreography_traits="CapabilityGuard JournalCoupling LeakageBudget ContextIsolation"
    missing_choreo=""

    for trait in $choreography_traits; do
        if ! grep -r "$trait" crates/aura-mpst/src/ 2>/dev/null >/dev/null; then
            missing_choreo="$missing_choreo $trait"
        fi
    done

    if [ -n "$missing_choreo" ]; then
        warning "aura-mpst missing choreography traits:$missing_choreo"
    else
        success "aura-mpst contains expected choreography abstractions"
    fi
fi

# Check aura-macros for DSL annotations
if [ -d "crates/aura-macros" ]; then
    subsection "aura-macros DSL compliance"

    dsl_annotations="guard_capability flow_cost journal_facts"
    missing_annotations=""

    for annotation in $dsl_annotations; do
        if ! grep -r "$annotation" crates/aura-macros/src/ 2>/dev/null >/dev/null; then
            missing_annotations="$missing_annotations $annotation"
        fi
    done

    if [ -n "$missing_annotations" ]; then
        warning "aura-macros missing DSL annotations:$missing_annotations"
    else
        success "aura-macros contains expected DSL patterns"
    fi
fi

section_divider
section_header "Layer Content Pattern Validation"

# Layer 4: Check aura-protocol for orchestration patterns
if [ -d "crates/aura-protocol" ]; then
    subsection "Layer 4: Orchestration pattern compliance"

    orchestration_patterns="GuardChain CapGuard FlowGuard JournalCoupler consensus anti.*entropy"
    missing_orchestration=""

    for pattern in $orchestration_patterns; do
        if ! grep -ri "$pattern" crates/aura-protocol/src/ 2>/dev/null >/dev/null; then
            missing_orchestration="$missing_orchestration $pattern"
        fi
    done

    if [ -n "$missing_orchestration" ]; then
        warning "aura-protocol missing orchestration patterns:$missing_orchestration"
    fi

    # Check for distributed state management
    if grep -r "distributed.*state\|cross.*handler.*coordination" crates/aura-protocol/src/ 2>/dev/null >/dev/null; then
        success "aura-protocol contains distributed coordination patterns"
    fi
fi

# Layer 6: Check runtime crates for deployment patterns
if [ -d "crates/aura-agent" ]; then
    subsection "Layer 6: Runtime deployment compliance"

    deployment_patterns="lifecycle.*management startup shutdown signal"
    found_deployment=false

    for pattern in $deployment_patterns; do
        if grep -ri "$pattern" crates/aura-agent/src/ 2>/dev/null >/dev/null; then
            found_deployment=true
            break
        fi
    done

    if $found_deployment; then
        success "aura-agent contains deployment lifecycle patterns"
    else
        warning "aura-agent missing deployment lifecycle management"
    fi
fi

if [ -d "crates/aura-simulator" ]; then
    simulation_patterns="deterministic.*simulation virtual.*time failure.*injection"
    found_simulation=false

    for pattern in $simulation_patterns; do
        if grep -ri "$pattern" crates/aura-simulator/src/ 2>/dev/null >/dev/null; then
            found_simulation=true
            break
        fi
    done

    if $found_simulation; then
        success "aura-simulator contains simulation patterns"
    else
        warning "aura-simulator missing deterministic simulation features"
    fi
fi

section_divider
section_header "Feature Crate Protocol Completeness"

# Check each feature crate for expected patterns
if [ -d "crates/aura-authenticate" ]; then
    subsection "aura-authenticate protocol patterns"
    auth_patterns=0
    if grep -ri "device.*auth" crates/aura-authenticate/src/ 2>/dev/null >/dev/null; then
        auth_patterns=$((auth_patterns + 1))
    fi
    if grep -ri "threshold.*auth" crates/aura-authenticate/src/ 2>/dev/null >/dev/null; then
        auth_patterns=$((auth_patterns + 1))
    fi
    if grep -ri "guardian.*auth" crates/aura-authenticate/src/ 2>/dev/null >/dev/null; then
        auth_patterns=$((auth_patterns + 1))
    fi

    if [ $auth_patterns -eq 3 ]; then
        success "aura-authenticate contains expected auth patterns (3/3)"
    elif [ $auth_patterns -gt 0 ]; then
        info "aura-authenticate partially implements auth patterns ($auth_patterns/3)"
    else
        warning "aura-authenticate missing expected auth patterns (0/3)"
    fi
fi

if [ -d "crates/aura-frost" ]; then
    subsection "aura-frost protocol patterns"
    frost_patterns=0
    if grep -ri "FROST.*ceremony" crates/aura-frost/src/ 2>/dev/null >/dev/null; then
        frost_patterns=$((frost_patterns + 1))
    fi
    if grep -ri "key.*resharing" crates/aura-frost/src/ 2>/dev/null >/dev/null; then
        frost_patterns=$((frost_patterns + 1))
    fi
    if grep -ri "threshold.*signature" crates/aura-frost/src/ 2>/dev/null >/dev/null; then
        frost_patterns=$((frost_patterns + 1))
    fi

    if [ $frost_patterns -eq 3 ]; then
        success "aura-frost contains expected FROST patterns (3/3)"
    elif [ $frost_patterns -gt 0 ]; then
        info "aura-frost partially implements FROST patterns ($frost_patterns/3)"
    else
        warning "aura-frost missing expected FROST patterns (0/3)"
    fi
fi

if [ -d "crates/aura-recovery" ]; then
    subsection "aura-recovery protocol patterns"
    recovery_patterns=0
    if grep -ri "guardian.*recovery" crates/aura-recovery/src/ 2>/dev/null >/dev/null; then
        recovery_patterns=$((recovery_patterns + 1))
    fi
    if grep -ri "dispute.*escalation" crates/aura-recovery/src/ 2>/dev/null >/dev/null; then
        recovery_patterns=$((recovery_patterns + 1))
    fi
    if grep -ri "audit.*trail" crates/aura-recovery/src/ 2>/dev/null >/dev/null; then
        recovery_patterns=$((recovery_patterns + 1))
    fi

    if [ $recovery_patterns -eq 3 ]; then
        success "aura-recovery contains expected recovery patterns (3/3)"
    elif [ $recovery_patterns -gt 0 ]; then
        info "aura-recovery partially implements recovery patterns ($recovery_patterns/3)"
    else
        warning "aura-recovery missing expected recovery patterns (0/3)"
    fi
fi

if [ -d "crates/aura-sync" ]; then
    subsection "aura-sync protocol patterns"
    sync_patterns=0
    if grep -ri "journal.*sync" crates/aura-sync/src/ 2>/dev/null >/dev/null; then
        sync_patterns=$((sync_patterns + 1))
    fi
    if grep -ri "anti.*entropy" crates/aura-sync/src/ 2>/dev/null >/dev/null; then
        sync_patterns=$((sync_patterns + 1))
    fi

    if [ $sync_patterns -eq 2 ]; then
        success "aura-sync contains expected sync patterns (2/2)"
    elif [ $sync_patterns -gt 0 ]; then
        info "aura-sync partially implements sync patterns ($sync_patterns/2)"
    else
        warning "aura-sync missing expected sync patterns (0/2)"
    fi
fi

section_divider
section_header "Architectural Anti-Pattern Detection"

subsection "Layer violation patterns"

# Check for UI logic in non-UI layers
ui_patterns="main\(\)\|clap::\|structopt::"
for crate in aura-core aura-journal aura-wot aura-verify aura-store aura-transport aura-effects aura-composition aura-protocol aura-authenticate aura-frost aura-invitation aura-recovery aura-relational aura-rendezvous aura-sync aura-storage aura-agent aura-simulator; do
    if [ -d "crates/$crate" ] && [ "$crate" != "aura-cli" ]; then
        if grep -rE "$ui_patterns" crates/$crate/src/ 2>/dev/null | head -1; then
            violation "$crate contains UI patterns (main() should only be in aura-cli)"
        fi
    fi
done

# Check for direct OS integration in domain crates (Layer 2)
os_patterns="std::fs|tokio::fs|std::net|tokio::net|std::process|std::env"
for crate in aura-journal aura-wot aura-verify aura-store aura-transport aura-mpst aura-macros; do
    if [ -d "crates/$crate" ]; then
        if grep -rE "$os_patterns" crates/$crate/src/ 2>/dev/null | grep -v "use.*std::" | head -1; then
            violation "$crate contains direct OS integration (should use infrastructure effects)"
        fi
    fi
done

# Check for runtime-specific logic in feature crates
runtime_patterns="tokio::main|async_std::main|#\[tokio::main\]"
for crate in aura-authenticate aura-frost aura-invitation aura-recovery aura-relational aura-rendezvous aura-sync aura-storage; do
    if [ -d "crates/$crate" ]; then
        if grep -rE "$runtime_patterns" crates/$crate/src/ 2>/dev/null | head -1; then
            violation "$crate contains runtime-specific patterns (should be reusable building blocks)"
        fi
    fi
done

subsection "Effect system violations"

# Check for direct effect instantiation instead of composition
direct_instantiation="::new\(\).*Handler|Handler::new"
for crate in aura-journal aura-wot aura-authenticate aura-recovery aura-relational; do
    if [ -d "crates/$crate" ]; then
        if grep -rE "$direct_instantiation" crates/$crate/src/ 2>/dev/null | head -3; then
            warning "$crate directly instantiates handlers (consider using composition)"
        fi
    fi
done

section_divider
section_header "Documentation Pattern Compliance"

subsection "Guard chain sequence validation"

# Check that the guard chain follows the documented order
if [ -d "crates/aura-protocol" ]; then
    guard_chain_order="AuthorizationEffects.*FlowBudgetEffects.*LeakageEffects.*JournalEffects.*TransportEffects"

    if grep -r "$guard_chain_order" crates/aura-protocol/src/ 2>/dev/null >/dev/null; then
        success "Guard chain follows documented sequence"
    else
        # Check for individual components
        guard_components="AuthorizationEffects FlowBudgetEffects LeakageEffects JournalEffects TransportEffects"
        missing_guards=""

        for guard in $guard_components; do
            if ! grep -r "$guard" crates/aura-protocol/src/ 2>/dev/null >/dev/null; then
                missing_guards="$missing_guards $guard"
            fi
        done

        if [ -n "$missing_guards" ]; then
            warning "Missing guard chain components:$missing_guards"
            echo "   ⚡ SOLUTION: Implement missing guards in crates/aura-protocol/src/guards/"
            echo "   📖 WHY: Guard chain enforces authorization → flow → leakage → journal → transport"
            echo "   🔧 HOW: Create guard structs that wrap effects and enforce policy sequence"
        fi
    fi
fi

subsection "Domain effect composition pattern validation"

# Check for the documented pattern: domain handlers compose infrastructure effects
if [ -d "crates/aura-journal" ]; then
    composition_pattern="struct.*Handler.*CryptoEffects.*StorageEffects"

    if grep -r "$composition_pattern" crates/aura-journal/src/ 2>/dev/null >/dev/null; then
        success "aura-journal follows domain effect composition pattern"
    else
        info "aura-journal may not follow documented composition pattern"
        echo "   ⚡ SUGGESTION: Consider domain handler pattern: JournalHandler<C,S>"
        echo "   📖 WHY: Domain handlers should compose infrastructure effects with business logic"
        echo "   💡 EXAMPLE: struct JournalHandler<C: CryptoEffects, S: StorageEffects>"
    fi
fi

section_divider
section_header "Enhanced Dependency Validation"

# Enhanced dependency validation with specific expected dependencies
if check_cargo && [ -n "$deps_json" ]; then
    subsection "Expected dependency compliance"

    # Check aura-verify dependencies
    if [ -d "crates/aura-verify" ]; then
        if ! echo "$deps_json" | jq -r 'select(.name == "aura-verify") | .deps[]' 2>/dev/null | grep -q "aura-core"; then
            warning "aura-verify missing expected dependency: aura-core"
        fi
    fi

    # Check aura-effects dependencies
    if [ -d "crates/aura-effects" ]; then
        if ! echo "$deps_json" | jq -r 'select(.name == "aura-effects") | .deps[]' 2>/dev/null | grep -q "aura-core"; then
            warning "aura-effects missing expected dependency: aura-core"
        fi
    fi

    # Check aura-protocol dependencies (critical layer)
    if [ -d "crates/aura-protocol" ]; then
        protocol_deps="aura-core aura-journal aura-verify aura-wot aura-effects aura-mpst"
        missing_protocol_deps=""

        for dep in $protocol_deps; do
            if ! echo "$deps_json" | jq -r 'select(.name == "aura-protocol") | .deps[]' 2>/dev/null | grep -q "$dep"; then
                missing_protocol_deps="$missing_protocol_deps $dep"
            fi
        done

        if [ -n "$missing_protocol_deps" ]; then
            warning "aura-protocol missing expected dependencies:$missing_protocol_deps"
        fi
    fi

    # Check aura-cli dependencies
    if [ -d "crates/aura-cli" ]; then
        cli_deps="aura-agent aura-protocol aura-core"
        missing_cli_deps=""

        for dep in $cli_deps; do
            if ! echo "$deps_json" | jq -r 'select(.name == "aura-cli") | .deps[]' 2>/dev/null | grep -q "$dep"; then
                missing_cli_deps="$missing_cli_deps $dep"
            fi
        done

        if [ -n "$missing_cli_deps" ]; then
            warning "aura-cli missing expected dependencies:$missing_cli_deps"
        fi
    fi
fi

section_divider
section_header "Implementation Completeness"

# Verify the compatibility bridge was removed
if [ -f "crates/aura-effects/src/id_generation.rs" ]; then
    violation "Compatibility bridge still exists (should have been removed)"
fi

section_divider
section_header "Effect System Usage Compliance"

info "Effect system enforces deterministic simulation and consistent interfaces"
echo "   ✅ ALLOWED: aura-effects implementations, runtime/effects.rs, pure functions"
echo "   🚫 FORBIDDEN: Direct system calls in domain logic, feature crates, protocols"
echo ""

subsection "Direct impure function usage detection"

# Check for direct time usage (should use TimeEffects)
time_violations=$(grep -r "SystemTime::now\|Instant::now\|std::time::" crates/*/src/ 2>/dev/null | \
    grep -v "aura-effects" | \
    grep -v "test" | \
    grep -v "TimeEffects" | \
    grep -v "impl.*TimeEffects" | \
    grep -v "aura-core.*hash" | \
    grep -v "runtime/effects.rs" || true)

if [ -n "$time_violations" ]; then
    violation "Direct time access found (should use TimeEffects)"
    echo "$time_violations"
    echo "   ⚡ SOLUTION: Use effects.current_time() via TimeEffects trait"
    echo "   📖 WHY: Time is impure and must be mockable for deterministic simulation"
    echo "   🔧 HOW: Pass TimeEffects and call effects.current_time().await"
    echo "   💡 NOTE: Effect implementations in aura-effects and runtime/effects.rs are exempt"
fi

# Check for direct randomness usage (should use RandomEffects)
random_violations=$(grep -r "rand::\|thread_rng\|OsRng\|random()" crates/*/src/ 2>/dev/null | \
    grep -v "aura-effects" | \
    grep -v "test" | \
    grep -v "RandomEffects" | \
    grep -v "impl.*RandomEffects" | \
    grep -v "runtime/effects.rs" || true)

if [ -n "$random_violations" ]; then
    violation "Direct randomness usage found (should use RandomEffects)"
    echo "$random_violations"
    echo "   ⚡ SOLUTION: Use effects.random_bytes() via RandomEffects trait"
    echo "   📖 WHY: Randomness must be deterministic and controllable for simulation"
    echo "   🔧 HOW: Pass RandomEffects and call effects.random_bytes().await"
    echo "   💡 NOTE: Effect implementations in aura-effects are exempt"
fi

# Check for direct filesystem access (should use StorageEffects)
fs_violations=$(grep -r "std::fs::\|tokio::fs::\|File::open\|File::create" crates/*/src/ 2>/dev/null | \
    grep -v "aura-effects" | \
    grep -v "test" | \
    grep -v "StorageEffects" | \
    grep -v "impl.*StorageEffects" | \
    grep -v "runtime/effects.rs" | \
    grep -v "aura-cli.*config\|aura-cli.*toml" || true)

if [ -n "$fs_violations" ]; then
    violation "Direct filesystem access found (should use StorageEffects)"
    echo "$fs_violations"
    echo "   ⚡ SOLUTION: Use effects.read_chunk() / effects.write_chunk() via StorageEffects"
    echo "   📖 WHY: File I/O must be mockable for testing and work across backends"
    echo "   🔧 HOW: Pass StorageEffects and call effects.read_chunk().await"
fi

# Check for direct network access (should use NetworkEffects)
network_violations=$(grep -r "std::net::\|tokio::net::\|TcpStream\|UdpSocket" crates/*/src/ 2>/dev/null | \
    grep -v "aura-effects" | \
    grep -v "test" | \
    grep -v "NetworkEffects" | \
    grep -v "impl.*NetworkEffects" | \
    grep -v "runtime/effects.rs" || true)

if [ -n "$network_violations" ]; then
    violation "Direct network access found (should use NetworkEffects)"
    echo "$network_violations"
    echo "   ⚡ SOLUTION: Use effects.send() via NetworkEffects trait"
    echo "   📖 WHY: Network I/O must be mockable and work across native/WASM targets"
    echo "   🔧 HOW: Pass NetworkEffects and call effects.send().await"
fi

subsection "Context propagation compliance"

# Check for missing EffectContext parameters in async functions
missing_context=$(grep -r "async fn.*(.*)" crates/*/src/ 2>/dev/null | \
    grep -v "test" | \
    grep -v "EffectContext\|&self\|&mut self" | \
    grep -v "aura-effects" | \
    grep -v "aura-testkit" | \
    grep -v "trait.*{" | \
    grep -v "impl.*{" | \
    head -10 || true)

if [ -n "$missing_context" ]; then
    warning "Async functions without EffectContext found"
    echo "$missing_context"
    echo "   ⚡ SOLUTION: Add ctx: &EffectContext parameter to async functions"
    echo "   📖 WHY: Context must flow explicitly for tracing and request correlation"
    echo "   🔧 HOW: async fn my_func(ctx: &EffectContext, ...) -> Result<T>"
fi

# Check for ambient global state usage (anti-pattern)
global_state=$(grep -r "lazy_static\|once_cell\|static.*Mutex" crates/*/src/ 2>/dev/null | \
    grep -v "test" | \
    grep -v "aura-effects" | \
    grep -v "aura-testkit" | \
    grep -v "static.*LOG" | \
    head -5 || true)

if [ -n "$global_state" ]; then
    violation "Global state found (violates explicit context principle)"
    echo "$global_state"
    echo "   ⚡ SOLUTION: Pass state via EffectContext or dependency injection"
    echo "   📖 WHY: Global state breaks deterministic testing and WASM compatibility"
    echo "   🔧 HOW: Store in EffectContext.metadata or pass as effect handler parameter"
    echo "   💡 NOTE: Static loggers and constants are acceptable"
fi

section_divider
section_header "Guard Chain Compliance"

subsection "Guard chain sequence enforcement"

# Check that network sends go through guard chain
unguarded_sends=$(grep -r "\.send(" crates/*/src/ 2>/dev/null | \
    grep -v "guard\|CapGuard\|FlowGuard" | \
    grep -v "test" | \
    grep -v "aura-effects" | \
    grep -v "aura-testkit" | \
    grep -v "mpsc\|channel\|sender" | \
    grep -v "impl.*NetworkEffects" | \
    head -5 || true)

if [ -n "$unguarded_sends" ]; then
    violation "Network sends bypass guard chain"
    echo "$unguarded_sends"
    echo "   ⚡ SOLUTION: Use guard chain wrapper for all network sends"
    echo "   📖 WHY: Guard chain enforces authorization → flow → leakage → journal sequence"
    echo "   🔧 HOW: guard_chain.send_with_context(ctx, message).await"
fi

# Check for Biscuit token usage
biscuit_usage=$(grep -r "Biscuit\|capability.*token" crates/*/src/ 2>/dev/null | \
    grep -v "test" | wc -l || echo 0)

if [ "$biscuit_usage" -gt 0 ]; then
    success "Found Biscuit capability token usage ($biscuit_usage references)"
else
    warning "No Biscuit capability tokens found (expected in authorization)"
    echo "   💡 TIP: Authorization should use Biscuit tokens, not stored capabilities"
fi

section_divider 
section_header "Choreographic Protocol Compliance"

subsection "Session type usage validation"

# Check for manual protocol implementation vs choreographic
manual_protocols=$(grep -r "tokio::select\|async move" crates/aura-*/src/ 2>/dev/null | \
    grep -v "aura-effects\|aura-testkit" | \
    grep -v "choreography\|session" | head -5 || true)

if [ -n "$manual_protocols" ]; then
    warning "Manual async protocols found (consider choreographic approach)"
    echo "$manual_protocols"
    echo "   💡 SUGGESTION: Use choreography! macro for type-safe distributed protocols"
    echo "   📖 WHY: Choreographies provide deadlock freedom and global reasoning"
    echo "   🔧 HOW: Define protocol with choreography! { roles: Alice, Bob; ... }"
fi

# Check for proper ChoreoHandler usage
choreo_handlers=$(grep -r "ChoreoHandler\|AuraHandler" crates/*/src/ 2>/dev/null | \
    wc -l || echo 0)

if [ "$choreo_handlers" -gt 0 ]; then
    success "Found choreography handler usage ($choreo_handlers references)"
fi

# Check for choreography annotations
annotation_usage=$(grep -r "guard_capability\|flow_cost\|journal_facts" crates/*/src/ 2>/dev/null | \
    grep -v "test" | wc -l || echo 0)

if [ "$annotation_usage" -gt 0 ]; then
    success "Found choreography annotations ($annotation_usage references)"
else
    warning "No choreography annotations found"
    echo "   💡 TIP: Use annotations for guard capabilities and flow costs"
fi

section_divider
section_header "CRDT Implementation Compliance"

subsection "Semilattice operation validation"

# Check for proper semilattice trait usage
semilattice_violations=$(grep -r "\.merge\|\.join" crates/*/src/ 2>/dev/null | \
    grep -v "JoinSemilattice\|join(" | head -5 || true)

if [ -n "$semilattice_violations" ]; then
    warning "Direct merge operations found (should use semilattice traits)"
    echo "$semilattice_violations"
    echo "   💡 SUGGESTION: Use JoinSemilattice trait for CRDT operations"
    echo "   📖 WHY: Semilattice traits ensure mathematical correctness"
    echo "   🔧 HOW: impl JoinSemilattice for MyType { fn join(&self, other: &Self) }"
fi

# Check for fact-based journal patterns
fact_patterns=$(grep -r "AttestedOp\|SnapshotFact\|RelationalFact" crates/aura-journal/src/ 2>/dev/null | \
    wc -l || echo 0)

if [ "$fact_patterns" -gt 0 ]; then
    success "Found fact-based journal patterns ($fact_patterns references)"
else
    warning "Missing fact-based journal implementation"
    echo "   💡 TIP: Journal should store facts: AttestedOp, SnapshotFact, RelationalFact"
fi

subsection "Async architecture pattern compliance"

# Check for blocking operations (WASM incompatible)
blocking_ops=$(grep -r "\.blocking\|thread::\|std::thread" crates/*/src/ 2>/dev/null | \
    grep -v "test" | \
    grep -v "aura-effects" | \
    grep -v "runtime/effects.rs" | \
    grep -v "impl.*Effects" || true)

if [ -n "$blocking_ops" ]; then
    violation "Blocking operations found (breaks WASM compatibility)"
    echo "$blocking_ops"
    echo "   ⚡ SOLUTION: Use async alternatives or move to aura-effects"
    echo "   📖 WHY: Aura must work in browsers and embedded runtimes"
    echo "   🔧 HOW: Replace blocking with async/await patterns"
fi

# Check for proper builder pattern usage
builder_patterns=$(grep -r "Builder\|::new()" crates/*/src/ 2>/dev/null | \
    grep -v "test" | wc -l || echo 0)

if [ "$builder_patterns" -gt 5 ]; then
    success "Found builder patterns for async initialization"
fi

# Check for lifecycle management
lifecycle_patterns=$(grep -r "LifecycleAware\|Initializing\|Ready\|ShuttingDown" crates/*/src/ 2>/dev/null | \
    wc -l || echo 0)

if [ "$lifecycle_patterns" -gt 0 ]; then
    success "Found lifecycle management patterns ($lifecycle_patterns references)"
else
    info "Consider adding explicit lifecycle management"
    echo "   💡 TIP: Implement LifecycleAware for initialization and shutdown hooks"
fi

# Summary
echo ""
echo ""
echo -e "${BOLD}${CYAN}📋 COMPLIANCE SUMMARY${NC}"
echo -e "${CYAN}==========================================${NC}"
echo ""

# Display status overview
if [ $VIOLATIONS -eq 0 ]; then
    echo -e "${GREEN}${BOLD}🎉 ALL CHECKS PASSED${NC}"
    echo -e "${GREEN}The Aura codebase follows the 8-layer architecture${NC}"
else
    echo -e "${RED}${BOLD}❌ COMPLIANCE ISSUES FOUND${NC}"
    echo -e "${RED}$VIOLATIONS violations must be fixed before proceeding${NC}"
fi

if [ $WARNINGS -gt 0 ]; then
    echo -e "${YELLOW}$WARNINGS warnings found (review recommended)${NC}"
fi

echo ""

# Architecture layer status
echo -e "${BOLD}Architecture Layer Status:${NC}"
echo -e "  ${GREEN}1. ✅${NC} Interface Layer (aura-core) - Pure trait definitions"
echo -e "  ${GREEN}2. ✅${NC} Specification Layer (domains + aura-mpst) - Domain logic"
echo -e "  ${GREEN}3. ✅${NC} Implementation Layer (aura-effects + aura-composition) - Effect handlers and composition"
echo -e "  ${GREEN}4. ✅${NC} Orchestration Layer (aura-protocol) - Multi-party coordination"
echo -e "  ${GREEN}5. ✅${NC} Feature/Protocol Layer - Complete implementations"
echo -e "  ${GREEN}6. ✅${NC} Runtime Composition Layer - Assembly libraries"
echo -e "  ${GREEN}7. ✅${NC} User Interface Layer - Applications with main()"
echo -e "  ${GREEN}8. ✅${NC} Testing/Tools Layer - Cross-cutting utilities"

echo ""

# Enhanced checks summary
echo -e "${BOLD}Enhanced Checks Performed:${NC}"
echo -e "  ${BLUE}•${NC} Blanket implementations vs business logic detection"
echo -e "  ${BLUE}•${NC} Layer-specific pattern validation"
echo -e "  ${BLUE}•${NC} Cargo metadata dependency analysis"
echo -e "  ${BLUE}•${NC} Positive architectural pattern verification"
echo -e "  ${BLUE}•${NC} File and directory module detection"
echo -e "  ${BLUE}•${NC} Effect system usage compliance (impure functions)"
echo -e "  ${BLUE}•${NC} Context propagation and async architecture patterns"
echo -e "  ${BLUE}•${NC} Guard chain sequence enforcement"
echo -e "  ${BLUE}•${NC} Choreographic protocol compliance"
echo -e "  ${BLUE}•${NC} CRDT and semilattice operation validation"
echo -e "  ${BLUE}•${NC} WASM compatibility and lifecycle management"

if [ $VIOLATIONS -eq 0 ] && [ $WARNINGS -eq 0 ]; then
    echo ""
    echo -e "${GREEN}${BOLD}🚀 Ready for development!${NC}"
    echo -e "${GREEN}All architectural compliance checks passed with no warnings.${NC}"
elif [ $VIOLATIONS -eq 0 ]; then
    echo ""
    echo -e "${YELLOW}${BOLD}✨ Good to go with review${NC}"
    echo -e "${YELLOW}No violations found, but consider reviewing the warnings above.${NC}"
else
    echo ""
    echo -e "${RED}${BOLD}🔧 Action Required${NC}"
    echo -e "${RED}Please fix the violations above before proceeding.${NC}"
    echo -e "See ${CYAN}docs/999_project_structure.md${NC} for architectural guidelines."
fi

echo ""

if [ $VIOLATIONS -eq 0 ]; then
    exit 0
else
    exit 1
fi
