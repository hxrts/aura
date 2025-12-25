#!/usr/bin/env bash
# Kani Bounded Model Checking Suite Runner
# Runs all consensus Kani harnesses and logs detailed output to file
# Only displays summary messages to console

set -euo pipefail

# Configuration
PACKAGE="aura-protocol"
UNWIND="10"
LOG_DIR="logs/kani"
LOG_FILE="${LOG_DIR}/kani-suite-$(date +%Y%m%d-%H%M%S).log"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# All harnesses to run
HARNESSES=(
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

# Create log directory
mkdir -p "$LOG_DIR"

# Counters
PASSED=0
FAILED=0
TOTAL=${#HARNESSES[@]}
FAILED_HARNESSES=()

# Header
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

# Log header
{
    echo "Kani Suite Run: $(date)"
    echo "Package: ${PACKAGE}"
    echo "Unwind bound: ${UNWIND}"
    echo "Harnesses: ${TOTAL}"
    echo ""
    echo "════════════════════════════════════════════════════════════════"
    echo ""
} >> "$LOG_FILE"

# Run each harness
for harness in "${HARNESSES[@]}"; do
    echo -ne "  ${YELLOW}○${NC} ${harness}... "

    # Log harness start
    {
        echo "────────────────────────────────────────────────────────────────"
        echo "Harness: ${harness}"
        echo "Started: $(date)"
        echo ""
    } >> "$LOG_FILE"

    # Run Kani and capture output
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

        # Log success
        {
            echo ""
            echo "Result: PASS"
            echo "Duration: ${DURATION}s"
            echo ""
        } >> "$LOG_FILE"
    else
        END_TIME=$(date +%s)
        DURATION=$((END_TIME - START_TIME))
        echo -e "\r  ${RED}✗${NC} ${harness} ${RED}[FAIL]${NC} (${DURATION}s)"
        ((FAILED++))
        FAILED_HARNESSES+=("$harness")

        # Log failure
        {
            echo ""
            echo "Result: FAIL"
            echo "Duration: ${DURATION}s"
            echo ""
        } >> "$LOG_FILE"
    fi
done

# Summary
echo ""
echo -e "${BLUE}════════════════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  Suite Complete${NC}"
echo -e "${BLUE}════════════════════════════════════════════════════════════════${NC}"
echo ""
echo -e "  Passed:  ${GREEN}${PASSED}${NC} / ${TOTAL}"
echo -e "  Failed:  ${RED}${FAILED}${NC} / ${TOTAL}"
echo ""

# Log summary
{
    echo "════════════════════════════════════════════════════════════════"
    echo "SUITE SUMMARY"
    echo "════════════════════════════════════════════════════════════════"
    echo ""
    echo "Passed: ${PASSED} / ${TOTAL}"
    echo "Failed: ${FAILED} / ${TOTAL}"
    echo ""
    if [ ${#FAILED_HARNESSES[@]} -gt 0 ]; then
        echo "Failed harnesses:"
        for h in "${FAILED_HARNESSES[@]}"; do
            echo "  - ${h}"
        done
    fi
    echo ""
    echo "Completed: $(date)"
} >> "$LOG_FILE"

# Final result
if [ "$FAILED" -eq 0 ]; then
    echo -e "  ${GREEN}SUCCESS${NC} - All harnesses verified"
    echo ""
    echo -e "  Full log: ${LOG_FILE}"
    echo ""
    exit 0
else
    echo -e "  ${RED}FAILURE${NC} - Some harnesses failed verification"
    echo ""
    echo -e "  Failed harnesses:"
    for h in "${FAILED_HARNESSES[@]}"; do
        echo -e "    - ${h}"
    done
    echo ""
    echo -e "  See ${LOG_FILE} for details"
    echo ""
    exit 1
fi
