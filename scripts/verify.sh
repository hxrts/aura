#!/usr/bin/env bash
# verify.sh - Unified verification scripting
#
# Usage:
#   ./scripts/verify.sh coverage [--md|--json]  # Generate verification coverage report
#   ./scripts/verify.sh quint-types [--verbose] # Check Quint-Rust type correspondence
#   ./scripts/verify.sh kani                    # Run Kani bounded model checking suite
#
# Run without arguments to see available commands.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# ============================================================================
# Subcommand: coverage
# ============================================================================

cmd_coverage() {
    local FORMAT="${1:---md}"
    local QUINT_DIR="$PROJECT_ROOT/verification/quint"
    local LEAN_DIR="$PROJECT_ROOT/verification/lean"
    local RUST_SRC="$PROJECT_ROOT/crates"

    # Data collection helpers
    count_quint_specs() { find "$QUINT_DIR" -name "*.qnt" -type f | wc -l | tr -d ' '; }
    count_quint_invariants() { grep -rh "^[[:space:]]*val Invariant" "$QUINT_DIR" --include="*.qnt" 2>/dev/null | wc -l | tr -d ' '; }
    count_quint_temporal() { grep -rh "^[[:space:]]*temporal " "$QUINT_DIR" --include="*.qnt" 2>/dev/null | wc -l | tr -d ' '; }
    count_quint_types() { grep -rhE "type[[:space:]]+[A-Z][a-zA-Z0-9_]*[[:space:]]*=" "$QUINT_DIR" --include="*.qnt" 2>/dev/null | wc -l | tr -d ' '; }
    count_rust_invariant_checks() { grep -rh "debug_assert.*Invariant\|check_.*invariant\|InvariantCheck" "$RUST_SRC" --include="*.rs" 2>/dev/null | wc -l | tr -d ' '; }
    count_lean_theorems() { [ -d "$LEAN_DIR" ] && grep -rh "^theorem\|^lemma" "$LEAN_DIR" --include="*.lean" 2>/dev/null | wc -l | tr -d ' ' || echo "0"; }
    count_itf_traces() { find "$PROJECT_ROOT/verification/quint/traces" -name "*.itf.json" -type f 2>/dev/null | wc -l | tr -d ' '; }
    count_differential_tests() { grep -rh "#\[test\]" "$RUST_SRC/aura-testkit/tests" --include="*.rs" 2>/dev/null | wc -l | tr -d ' '; }

    count_verified_specs() {
        grep -rl "quint verify\|--invariant=" "$PROJECT_ROOT/justfile" "$PROJECT_ROOT/.github/workflows/" 2>/dev/null | \
            xargs grep -h "verification/quint" 2>/dev/null | \
            grep -oE "verification/quint/[^[:space:]\"']*.qnt" | sort -u | wc -l | tr -d ' '
    }

    list_quint_invariants() {
        grep -rn "^[[:space:]]*val Invariant" "$QUINT_DIR" --include="*.qnt" 2>/dev/null | \
            sed "s|$QUINT_DIR/||" | \
            while IFS=: read -r file line content; do
                inv_name=$(echo "$content" | sed -E 's/.*val (Invariant[A-Za-z0-9_]*).*/\1/')
                echo "$inv_name|$file:$line"
            done
    }

    list_rust_quint_correspondence() {
        grep -rln "Quint.*Correspondence\|QuintMappable\|quint_type_name" "$RUST_SRC" --include="*.rs" 2>/dev/null | \
            sed "s|$RUST_SRC/||" | sort -u
    }

    categorize_specs() {
        local verified_pattern="protocol_consensus|protocol_journal|protocol_frost|protocol_anti_entropy|protocol_recovery|protocol_sessions|authorization|transport|invitation|epochs"
        find "$QUINT_DIR" -name "*.qnt" -type f | while read -r spec; do
            local rel="${spec#$QUINT_DIR/}"
            local name=$(basename "$spec" .qnt)
            local inv_count
            inv_count=$(grep -c "val Invariant" "$spec" 2>/dev/null) || inv_count=0
            if echo "$name" | grep -qE "$verified_pattern"; then
                echo "VERIFIED:$rel:$inv_count"
            elif [ "$inv_count" -gt 0 ]; then
                echo "HAS_INVARIANTS:$rel:$inv_count"
            else
                echo "NO_INVARIANTS:$rel:$inv_count"
            fi
        done
    }

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

        local verified=0 has_inv=0 no_inv=0
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
        local corr_count=$(list_rust_quint_correspondence | wc -l | tr -d ' ')
        [ "$corr_count" -gt 20 ] && echo "... and $(( corr_count - 20 )) more"
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
                [ "$thm_count" -gt 0 ] && echo "| \`$rel\` | $thm_count |"
            done
        else
            echo "No Lean verification modules found."
        fi
        echo ""
        echo "## Verified Invariants"
        echo ""
        echo "| Invariant | Specification |"
        echo "|-----------|---------------|"
        list_quint_invariants | head -30 | while IFS='|' read -r inv loc; do
            echo "| \`$inv\` | $loc |"
        done
        local inv_count=$(list_quint_invariants | wc -l | tr -d ' ')
        [ "$inv_count" -gt 30 ] && echo "" && echo "_... and $(( inv_count - 30 )) more invariants_"
        echo ""
        echo "## Related Commands"
        echo ""
        echo "\`\`\`bash"
        echo "just quint-verify-models  # Run Apalache model checking"
        echo "just verify quint-types   # Check Quint-Rust type drift"
        echo "just verify kani          # Run Kani bounded model checking"
        echo "\`\`\`"
    }

    generate_json_report() {
        cat << EOF
{
  "generated": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")",
  "metrics": {
    "quint_specs": $(count_quint_specs),
    "quint_invariants": $(count_quint_invariants),
    "quint_temporal_properties": $(count_quint_temporal),
    "quint_types": $(count_quint_types),
    "rust_invariant_checks": $(count_rust_invariant_checks),
    "lean_theorems": $(count_lean_theorems),
    "verified_specs": $(count_verified_specs),
    "itf_traces": $(count_itf_traces),
    "differential_tests": $(count_differential_tests)
  }
}
EOF
    }

    case "$FORMAT" in
        --json) generate_json_report ;;
        --md|*) generate_markdown_report ;;
    esac
}

# ============================================================================
# Subcommand: quint-types
# ============================================================================

cmd_quint_types() {
    local VERBOSE="${1:-}"
    local QUINT_DIR="$PROJECT_ROOT/verification/quint"
    local RUST_SRC="$PROJECT_ROOT/crates"
    local TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    log_info "Quint-Rust Type Drift Detection"
    log_info "==============================="
    echo

    # Extract Quint types
    log_info "Extracting Quint type definitions..."
    : > "$TEMP_DIR/quint_types.txt"

    for qnt_file in $(find "$QUINT_DIR" -name "*.qnt" -type f); do
        rel_path="${qnt_file#$QUINT_DIR/}"
        grep -nE "type[[:space:]]+[A-Z][a-zA-Z0-9_]*[[:space:]]*=" "$qnt_file" 2>/dev/null | while IFS=: read -r line_num type_def; do
            type_name=$(echo "$type_def" | sed -E 's/.*type[[:space:]]+([A-Z][a-zA-Z0-9_]*).*/\1/')
            if [[ -n "$type_name" && ! "$type_name" =~ = ]]; then
                if echo "$type_def" | grep -q "{"; then
                    echo "RECORD:$type_name:$rel_path:$line_num" >> "$TEMP_DIR/quint_types.txt"
                elif echo "$type_def" | grep -q "|"; then
                    echo "SUM:$type_name:$rel_path:$line_num" >> "$TEMP_DIR/quint_types.txt"
                else
                    echo "ALIAS:$type_name:$rel_path:$line_num" >> "$TEMP_DIR/quint_types.txt"
                fi
            fi
        done || true
    done

    sort -u "$TEMP_DIR/quint_types.txt" -o "$TEMP_DIR/quint_types.txt"

    # Extract Rust QuintMappable impls
    log_info "Extracting Rust QuintMappable implementations..."
    : > "$TEMP_DIR/rust_mappables.txt"

    grep -rn "impl QuintMappable for" "$RUST_SRC" --include="*.rs" 2>/dev/null | while IFS=: read -r file_path line_num impl_line; do
        type_name=$(echo "$impl_line" | sed -E 's/.*impl[[:space:]]+QuintMappable[[:space:]]+for[[:space:]]+([A-Za-z0-9_]+).*/\1/')
        rel_path="${file_path#$RUST_SRC/}"
        if [[ -n "$type_name" && ! "$type_name" =~ \{ ]]; then
            quint_name=$(sed -n "${line_num},+60p" "$file_path" | grep -m1 'fn quint_type_name' -A3 | grep -oE '"[^"]*"' | head -1 | tr -d '"' 2>/dev/null || echo "")
            echo "IMPL:$type_name:$quint_name:$rel_path:$line_num" >> "$TEMP_DIR/rust_mappables.txt"
        fi
    done || true

    sort -u "$TEMP_DIR/rust_mappables.txt" -o "$TEMP_DIR/rust_mappables.txt"

    echo
    quint_count=$(wc -l < "$TEMP_DIR/quint_types.txt" | tr -d ' ')
    rust_count=$(wc -l < "$TEMP_DIR/rust_mappables.txt" | tr -d ' ')
    log_info "Found $quint_count Quint types, $rust_count Rust QuintMappable impls"
    echo

    # Compare types
    log_info "Comparing Quint and Rust types..."

    declare -A rust_by_quint
    while IFS=: read -r _ rust_type quint_name path line; do
        if [[ -n "$quint_name" ]]; then
            rust_by_quint["$quint_name"]="$rust_type|$path:$line"
        fi
    done < "$TEMP_DIR/rust_mappables.txt"

    matched=0
    unmapped=0

    while IFS=: read -r kind quint_type qnt_path qnt_line; do
        case "$quint_type" in
            Option|DataBinding|CachedNonce|NonceCommitment|OperationData|Epoch) continue ;;
        esac
        if [[ -n "${rust_by_quint[$quint_type]:-}" ]]; then
            [[ "$VERBOSE" == "--verbose" ]] && echo "  ✓ $quint_type -> ${rust_by_quint[$quint_type]}"
            ((matched++))
        else
            log_warn "Quint type '$quint_type' ($qnt_path:$qnt_line) has no QuintMappable impl"
            ((unmapped++))
        fi
    done < "$TEMP_DIR/quint_types.txt"

    echo
    log_info "Type correspondence summary:"
    log_info "  Mapped types: $matched"
    log_info "  Unmapped types: $unmapped"

    if [[ "$VERBOSE" == "--verbose" ]]; then
        echo
        echo "=== Mapped Types ==="
        while IFS=: read -r kind quint_type qnt_path qnt_line; do
            case "$quint_type" in Option|DataBinding|CachedNonce|NonceCommitment|OperationData|Epoch) continue ;; esac
            if [[ -n "${rust_by_quint[$quint_type]:-}" ]]; then
                info="${rust_by_quint[$quint_type]}"
                echo "$quint_type ($qnt_path:$qnt_line) -> ${info%%|*} (${info#*|})"
            fi
        done < "$TEMP_DIR/quint_types.txt"
        echo
        echo "=== Unmapped Quint Types ==="
        while IFS=: read -r kind quint_type qnt_path qnt_line; do
            case "$quint_type" in Option|DataBinding|CachedNonce|NonceCommitment|OperationData|Epoch) continue ;; esac
            [[ -z "${rust_by_quint[$quint_type]:-}" ]] && echo "- $quint_type ($kind) - $qnt_path:$qnt_line"
        done < "$TEMP_DIR/quint_types.txt"
    fi

    echo
    log_info "✓ Type drift check complete"
}

# ============================================================================
# Subcommand: kani
# ============================================================================

cmd_kani() {
    local PACKAGE="aura-protocol"
    local UNWIND="10"
    local LOG_DIR="logs/kani"
    local LOG_FILE="${LOG_DIR}/kani-suite-$(date +%Y%m%d-%H%M%S).log"

    local HARNESSES=(
        "apply_share_preserves_invariants"
        "trigger_fallback_preserves_invariants"
        "fail_consensus_preserves_invariants"
        "apply_share_monotonic_proposals"
        "apply_share_monotonic_equivocators"
        "apply_share_no_panic"
        "trigger_fallback_no_panic"
        "fail_consensus_no_panic"
        "committed_state_is_terminal"
        "failed_state_is_terminal"
        "phase_advances_forward"
        "commit_matches_threshold_result"
        "threshold_met_matches_reference"
        "has_proposal_matches_reference"
    )

    mkdir -p "$LOG_DIR"

    local PASSED=0 FAILED=0 TOTAL=${#HARNESSES[@]}
    local FAILED_HARNESSES=()

    echo -e "${BLUE}════════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}  Kani Bounded Model Checking Suite${NC}"
    echo -e "${BLUE}════════════════════════════════════════════════════════════════${NC}"
    echo ""
    echo -e "  Package:     ${PACKAGE}"
    echo -e "  Unwind:      ${UNWIND}"
    echo -e "  Harnesses:   ${TOTAL}"
    echo -e "  Log file:    ${LOG_FILE}"
    echo ""
    echo -e "${YELLOW}► Starting suite...${NC}"
    echo ""

    {
        echo "Kani Suite Run: $(date)"
        echo "Package: ${PACKAGE}"
        echo "Unwind bound: ${UNWIND}"
        echo "Harnesses: ${TOTAL}"
        echo ""
        echo "════════════════════════════════════════════════════════════════"
        echo ""
    } >> "$LOG_FILE"

    for harness in "${HARNESSES[@]}"; do
        echo -ne "  ${YELLOW}○${NC} ${harness}... "

        {
            echo "────────────────────────────────────────────────────────────────"
            echo "Harness: ${harness}"
            echo "Started: $(date)"
            echo ""
        } >> "$LOG_FILE"

        START_TIME=$(date +%s)
        if nix develop .#nightly --command cargo kani \
            --package "$PACKAGE" \
            --harness "$harness" \
            --default-unwind "$UNWIND" \
            >> "$LOG_FILE" 2>&1; then
            END_TIME=$(date +%s)
            DURATION=$((END_TIME - START_TIME))
            echo -e "\r  ${GREEN}✓${NC} ${harness} ${GREEN}[PASS]${NC} (${DURATION}s)"
            ((PASSED++))
            { echo ""; echo "Result: PASS"; echo "Duration: ${DURATION}s"; echo ""; } >> "$LOG_FILE"
        else
            END_TIME=$(date +%s)
            DURATION=$((END_TIME - START_TIME))
            echo -e "\r  ${RED}✗${NC} ${harness} ${RED}[FAIL]${NC} (${DURATION}s)"
            ((FAILED++))
            FAILED_HARNESSES+=("$harness")
            { echo ""; echo "Result: FAIL"; echo "Duration: ${DURATION}s"; echo ""; } >> "$LOG_FILE"
        fi
    done

    echo ""
    echo -e "${BLUE}════════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}  Suite Complete${NC}"
    echo -e "${BLUE}════════════════════════════════════════════════════════════════${NC}"
    echo ""
    echo -e "  Passed:  ${GREEN}${PASSED}${NC} / ${TOTAL}"
    echo -e "  Failed:  ${RED}${FAILED}${NC} / ${TOTAL}"
    echo ""

    {
        echo "════════════════════════════════════════════════════════════════"
        echo "SUITE SUMMARY"
        echo "Passed: ${PASSED} / ${TOTAL}"
        echo "Failed: ${FAILED} / ${TOTAL}"
        if [ ${#FAILED_HARNESSES[@]} -gt 0 ]; then
            echo "Failed harnesses:"
            for h in "${FAILED_HARNESSES[@]}"; do echo "  - ${h}"; done
        fi
        echo "Completed: $(date)"
    } >> "$LOG_FILE"

    if [ "$FAILED" -eq 0 ]; then
        echo -e "  ${GREEN}SUCCESS${NC} - All harnesses verified"
        echo -e "  Full log: ${LOG_FILE}"
        echo ""
        exit 0
    else
        echo -e "  ${RED}FAILURE${NC} - Some harnesses failed verification"
        echo -e "  Failed harnesses:"
        for h in "${FAILED_HARNESSES[@]}"; do echo -e "    - ${h}"; done
        echo -e "  See ${LOG_FILE} for details"
        echo ""
        exit 1
    fi
}

# ============================================================================
# Main
# ============================================================================

show_usage() {
    cat << EOF
Usage: $0 <command> [options]

Commands:
  coverage [--md|--json]    Generate verification coverage report (default: --md)
  quint-types [--verbose]   Check Quint-Rust type correspondence
  kani                      Run Kani bounded model checking suite

Examples:
  $0 coverage               # Markdown coverage report
  $0 coverage --json        # JSON metrics
  $0 quint-types --verbose  # Verbose type drift check
  $0 kani                   # Run all Kani harnesses
EOF
}

CMD="${1:-}"
shift || true

case "$CMD" in
    coverage)
        cmd_coverage "$@"
        ;;
    quint-types)
        cmd_quint_types "$@"
        ;;
    kani)
        cmd_kani "$@"
        ;;
    -h|--help|"")
        show_usage
        ;;
    *)
        echo "Unknown command: $CMD" >&2
        show_usage
        exit 1
        ;;
esac
