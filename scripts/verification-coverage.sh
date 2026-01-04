#!/usr/bin/env bash
# verification-coverage.sh - Generate verification coverage report
#
# This script analyzes the codebase to track verification coverage:
# - Rust modules with Quint specifications
# - Quint invariants with Rust debug assertions
# - Lean theorems with Quint correspondence
# - Overall verification coverage metrics
#
# Usage: ./scripts/verification-coverage.sh [--md | --json]
#
# Output formats:
#   --md    Generate Markdown report (default)
#   --json  Generate JSON metrics

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FORMAT="${1:---md}"

QUINT_DIR="$PROJECT_ROOT/verification/quint"
LEAN_DIR="$PROJECT_ROOT/verification/lean"
RUST_SRC="$PROJECT_ROOT/crates"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ============================================================================
# Data Collection
# ============================================================================

# Count Quint specifications
count_quint_specs() {
    find "$QUINT_DIR" -name "*.qnt" -type f | wc -l | tr -d ' '
}

# Count Quint invariants
count_quint_invariants() {
    grep -rh "^[[:space:]]*val Invariant" "$QUINT_DIR" --include="*.qnt" 2>/dev/null | wc -l | tr -d ' '
}

# Count Quint temporal properties
count_quint_temporal() {
    grep -rh "^[[:space:]]*temporal " "$QUINT_DIR" --include="*.qnt" 2>/dev/null | wc -l | tr -d ' '
}

# Count Quint type definitions
count_quint_types() {
    grep -rhE "type[[:space:]]+[A-Z][a-zA-Z0-9_]*[[:space:]]*=" "$QUINT_DIR" --include="*.qnt" 2>/dev/null | wc -l | tr -d ' '
}

# List Quint invariants with locations
list_quint_invariants() {
    grep -rn "^[[:space:]]*val Invariant" "$QUINT_DIR" --include="*.qnt" 2>/dev/null | \
        sed "s|$QUINT_DIR/||" | \
        while IFS=: read -r file line content; do
            inv_name=$(echo "$content" | sed -E 's/.*val (Invariant[A-Za-z0-9_]*).*/\1/')
            echo "$inv_name|$file:$line"
        done
}

# Count Rust debug assertions referencing Quint invariants
count_rust_invariant_checks() {
    grep -rh "debug_assert.*Invariant\|check_.*invariant\|InvariantCheck" "$RUST_SRC" --include="*.rs" 2>/dev/null | wc -l | tr -d ' '
}

# List Rust files with Quint correspondence comments
list_rust_quint_correspondence() {
    grep -rln "Quint.*Correspondence\|QuintMappable\|quint_type_name" "$RUST_SRC" --include="*.rs" 2>/dev/null | \
        sed "s|$RUST_SRC/||" | sort -u
}

# Count Lean 4 theorems
count_lean_theorems() {
    if [ -d "$LEAN_DIR" ]; then
        grep -rh "^theorem\|^lemma" "$LEAN_DIR" --include="*.lean" 2>/dev/null | wc -l | tr -d ' '
    else
        echo "0"
    fi
}

# List Lean theorems with correspondence comments
list_lean_quint_correspondence() {
    if [ -d "$LEAN_DIR" ]; then
        grep -rn "Quint:\|-- Corresponds to" "$LEAN_DIR" --include="*.lean" 2>/dev/null | \
            sed "s|$LEAN_DIR/||"
    fi
}

# Count verified Quint specs (those with Apalache checks)
count_verified_specs() {
    grep -rl "quint verify\|--invariant=" "$PROJECT_ROOT/justfile" "$PROJECT_ROOT/.github/workflows/" 2>/dev/null | \
        xargs grep -h "verification/quint" 2>/dev/null | \
        grep -oE "verification/quint/[^[:space:]\"']*.qnt" | sort -u | wc -l | tr -d ' '
}

# Count ITF trace coverage
count_itf_traces() {
    find "$PROJECT_ROOT/verification/quint/traces" -name "*.itf.json" -type f 2>/dev/null | wc -l | tr -d ' '
}

# Count differential test coverage
count_differential_tests() {
    grep -rh "#\[test\]" "$RUST_SRC/aura-testkit/tests" --include="*.rs" 2>/dev/null | wc -l | tr -d ' '
}

# List specs by verification status
categorize_specs() {
    local verified_pattern="protocol_consensus|protocol_journal|protocol_frost|protocol_anti_entropy|protocol_recovery|protocol_sessions|authorization|transport|invitation|epochs"

    find "$QUINT_DIR" -name "*.qnt" -type f | while read -r spec; do
        local rel="${spec#$QUINT_DIR/}"
        local name=$(basename "$spec" .qnt)

        # Check if spec has invariants
        local inv_count
        inv_count=$(grep -c "val Invariant" "$spec" 2>/dev/null) || inv_count=0

        # Check if spec is in CI verification
        if echo "$name" | grep -qE "$verified_pattern"; then
            echo "VERIFIED:$rel:$inv_count"
        elif [ "$inv_count" -gt 0 ]; then
            echo "HAS_INVARIANTS:$rel:$inv_count"
        else
            echo "NO_INVARIANTS:$rel:$inv_count"
        fi
    done
}

# ============================================================================
# Report Generation
# ============================================================================

generate_markdown_report() {
    local quint_specs=$(count_quint_specs)
    local quint_invariants=$(count_quint_invariants)
    local quint_temporal=$(count_quint_temporal)
    local quint_types=$(count_quint_types)
    local rust_checks=$(count_rust_invariant_checks)
    local lean_theorems=$(count_lean_theorems)
    local verified_specs=$(count_verified_specs)
    local itf_traces=$(count_itf_traces)
    local diff_tests=$(count_differential_tests)

    cat << EOF
# Verification Coverage Report

Generated: $(date -u +"%Y-%m-%d %H:%M:%S UTC")

## Summary Metrics

| Metric | Count |
|--------|-------|
| Quint Specifications | $quint_specs |
| Quint Invariants | $quint_invariants |
| Quint Temporal Properties | $quint_temporal |
| Quint Type Definitions | $quint_types |
| Rust Invariant Checks | $rust_checks |
| Lean Theorems | $lean_theorems |
| Verified Specs (CI) | $verified_specs |
| ITF Traces | $itf_traces |
| Differential Tests | $diff_tests |

## Verification Layers

### Layer 1: Quint Specifications

Formal protocol specifications in \`verification/quint/\`:

EOF

    # Categorize specs
    local verified=0
    local has_inv=0
    local no_inv=0

    while IFS=: read -r status path inv_count; do
        case "$status" in
            VERIFIED) ((verified++)) ;;
            HAS_INVARIANTS) ((has_inv++)) ;;
            NO_INVARIANTS) ((no_inv++)) ;;
        esac
    done < <(categorize_specs)

    echo "| Status | Count |"
    echo "|--------|-------|"
    echo "| Model-checked (CI verified) | $verified |"
    echo "| Has invariants (not in CI) | $has_inv |"
    echo "| No invariants (helpers/harness) | $no_inv |"
    echo ""

    echo "### Layer 2: Rust Integration"
    echo ""
    echo "Files with Quint type correspondence:"
    echo ""
    echo "\`\`\`"
    list_rust_quint_correspondence | head -20
    if [ $(list_rust_quint_correspondence | wc -l) -gt 20 ]; then
        echo "... and $(( $(list_rust_quint_correspondence | wc -l) - 20 )) more"
    fi
    echo "\`\`\`"
    echo ""

    echo "### Layer 3: Lean Proofs"
    echo ""
    if [ -d "$LEAN_DIR" ]; then
        echo "Lean 4 verification modules in \`verification/lean/\`:"
        echo ""
        echo "| Module | Theorems |"
        echo "|--------|----------|"
        find "$LEAN_DIR" -name "*.lean" -type f | while read -r lean_file; do
            local rel="${lean_file#$LEAN_DIR/}"
            local thm_count
            thm_count=$(grep -cE "^theorem|^lemma" "$lean_file" 2>/dev/null) || thm_count=0
            if [ "$thm_count" -gt 0 ]; then
                echo "| \`$rel\` | $thm_count |"
            fi
        done
    else
        echo "No Lean verification modules found."
    fi
    echo ""

    echo "## Verified Invariants"
    echo ""
    echo "Quint invariants with Apalache verification in CI:"
    echo ""
    echo "| Invariant | Specification |"
    echo "|-----------|---------------|"
    list_quint_invariants | head -30 | while IFS='|' read -r inv loc; do
        echo "| \`$inv\` | $loc |"
    done

    if [ $(list_quint_invariants | wc -l) -gt 30 ]; then
        echo ""
        echo "_... and $(( $(list_quint_invariants | wc -l) - 30 )) more invariants_"
    fi
    echo ""

    echo "## Coverage Recommendations"
    echo ""
    echo "### High Priority"
    echo ""
    echo "1. **Add CI verification for specs with invariants but no model checking:**"
    categorize_specs | grep "HAS_INVARIANTS" | head -5 | while IFS=: read -r _ path inv_count; do
        echo "   - \`$path\` ($inv_count invariants)"
    done
    echo ""
    echo "2. **Add QuintMappable implementations for core types used in ITF conformance:**"
    echo "   - ConsensusId, ResultId, ThresholdSignature"
    echo ""
    echo "3. **Expand Lean theorem coverage:**"
    echo "   - Add proofs for liveness properties"
    echo "   - Add proofs for cross-protocol safety"
    echo ""

    echo "## Related Commands"
    echo ""
    echo "\`\`\`bash"
    echo "just quint-verify-models        # Run Apalache model checking"
    echo "just quint-check-types          # Check Quint-Rust type drift"
    echo "just verify-conformance         # Run ITF conformance tests"
    echo "just verify-lean                # Build and check Lean proofs"
    echo "just verify-all                 # Run all verification"
    echo "\`\`\`"
}

generate_json_report() {
    local quint_specs=$(count_quint_specs)
    local quint_invariants=$(count_quint_invariants)
    local quint_temporal=$(count_quint_temporal)
    local quint_types=$(count_quint_types)
    local rust_checks=$(count_rust_invariant_checks)
    local lean_theorems=$(count_lean_theorems)
    local verified_specs=$(count_verified_specs)
    local itf_traces=$(count_itf_traces)
    local diff_tests=$(count_differential_tests)

    cat << EOF
{
  "generated": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "metrics": {
    "quint_specs": $quint_specs,
    "quint_invariants": $quint_invariants,
    "quint_temporal_properties": $quint_temporal,
    "quint_types": $quint_types,
    "rust_invariant_checks": $rust_checks,
    "lean_theorems": $lean_theorems,
    "verified_specs": $verified_specs,
    "itf_traces": $itf_traces,
    "differential_tests": $diff_tests
  }
}
EOF
}

# ============================================================================
# Main
# ============================================================================

case "$FORMAT" in
    --json)
        generate_json_report
        ;;
    --md|*)
        generate_markdown_report
        ;;
esac
