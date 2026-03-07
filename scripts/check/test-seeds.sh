#!/usr/bin/env bash
# Check deterministic test seed policy for AuraEffectSystem construction.
#
# Policy:
# 1) Tests must use AuraEffectSystem::simulation_for_test* helpers.
# 2) Tests must not call legacy testing/simulation constructors directly.
# 3) Runtime infrastructure may use explicit simulation constructors with local
#    allow attributes and rationale comments.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

QUIET_SUCCESS=false
if [[ "${AURA_CHECK_ARCH_MODE:-0}" == "1" ]]; then
    QUIET_SUCCESS=true
fi

is_test_context() {
    local file="$1"
    local lineno="$2"

    if [[ "$file" == *"/tests/"* || "$file" == *"_test.rs" || "$file" == *"test.rs" ]]; then
        return 0
    fi

    if grep -q "#\[cfg(test)\]" "$file" 2>/dev/null; then
        local cfg_line
        cfg_line=$(grep -n "#\[cfg(test)\]" "$file" 2>/dev/null | head -1 | cut -d: -f1)
        if [[ -n "$cfg_line" && "$lineno" =~ ^[0-9]+$ && "$lineno" -gt "$cfg_line" ]]; then
            return 0
        fi
    fi

    return 1
}

if ! $QUIET_SUCCESS; then
    echo "Checking deterministic test seed policy..."
fi

banned_pattern='AuraEffectSystem::(testing\(|testing_for_authority\(|testing_with_shared_transport\(|simulation\(|simulation_for_authority\(|simulation_with_shared_transport_for_authority\()'
all_banned_calls=$(grep -rnE "$banned_pattern" crates --include="*.rs" || true)

banned_in_tests=""
banned_outside_tests=""

while IFS= read -r line; do
    [[ -z "$line" ]] && continue

    file="${line%%:*}"
    rest="${line#*:}"
    lineno="${rest%%:*}"
    content="${rest#*:}"

    # Skip commented-only hits
    if [[ "$content" =~ ^[[:space:]]*// ]]; then
        continue
    fi

    if is_test_context "$file" "$lineno"; then
        banned_in_tests+="$line"$'\n'
    else
        banned_outside_tests+="$line"$'\n'
    fi
done <<< "$all_banned_calls"

helper_pattern='AuraEffectSystem::(simulation_for_test\(|simulation_for_test_with_salt\(|simulation_for_named_test\(|simulation_for_named_test_with_salt\(|simulation_for_test_for_authority\(|simulation_for_test_for_authority_with_salt\(|simulation_for_test_with_shared_transport\(|simulation_for_test_with_shared_transport_for_authority\()'
all_helper_calls=$(grep -rnE "$helper_pattern" crates --include="*.rs" || true)
helper_in_tests=""
while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    file="${line%%:*}"
    rest="${line#*:}"
    lineno="${rest%%:*}"
    content="${rest#*:}"
    if [[ "$content" =~ ^[[:space:]]*// ]]; then
        continue
    fi
    if is_test_context "$file" "$lineno"; then
        helper_in_tests+="$line"$'\n'
    fi
done <<< "$all_helper_calls"

status=0

if [[ -n "$banned_in_tests" ]]; then
    count=$(printf '%s' "$banned_in_tests" | sed '/^$/d' | wc -l | tr -d ' ')
    echo -e "${RED}ERROR: Found ${count} banned AuraEffectSystem constructor call(s) in test context:${NC}"
    if $QUIET_SUCCESS; then
        echo "  (run scripts/check/test-seeds.sh directly to see full locations)"
    else
        echo "$banned_in_tests" | sed '/^$/d' | head -20
        if [[ "$count" -gt 20 ]]; then
            echo "  ... and $((count - 20)) more"
        fi
    fi
    echo ""
    echo "Use AuraEffectSystem::simulation_for_test* helpers instead."
    status=1
fi

if [[ -n "$banned_outside_tests" ]]; then
    count=$(printf '%s' "$banned_outside_tests" | sed '/^$/d' | wc -l | tr -d ' ')
    echo -e "${YELLOW}WARNING: Found ${count} banned-constructor call(s) outside test context:${NC}"
    if $QUIET_SUCCESS; then
        echo "  (runtime infrastructure may allow these with explicit clippy allowances)"
    else
        echo "$banned_outside_tests" | sed '/^$/d' | head -10
        if [[ "$count" -gt 10 ]]; then
            echo "  ... and $((count - 10)) more"
        fi
    fi
fi

if [[ -z "$helper_in_tests" ]]; then
    echo -e "${YELLOW}WARNING: No simulation_for_test* helper calls found in test contexts.${NC}"
else
    helper_count=$(printf '%s' "$helper_in_tests" | sed '/^$/d' | wc -l | tr -d ' ')
    if ! $QUIET_SUCCESS; then
        echo -e "${GREEN}✓ Found ${helper_count} simulation_for_test* helper call(s) in test context${NC}"
    fi
fi

exit $status
