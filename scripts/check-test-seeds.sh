#!/usr/bin/env bash
# Check that all tests using AuraEffectSystem use unique deterministic seeds
#
# This script ensures test isolation by verifying that each test using
# simulation or testing mode with seeds uses a unique seed value.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "Checking for unique test seeds..."

# Find all simulation() calls with seeds
# We check all files because tests can be embedded in modules (e.g., #[cfg(test)] mod tests)
all_seeds=$(grep -rn "AuraEffectSystem::simulation.*,[[:space:]]*[0-9]" crates --include="*.rs" || true)

if [ -z "$all_seeds" ]; then
    echo -e "${GREEN}✓ No test seeds found (tests may be using other patterns)${NC}"
    exit 0
fi

# Extract seeds and check for duplicates
declare -A seed_map
duplicates_found=0

while IFS= read -r line; do
    if [[ -z "$line" ]]; then
        continue
    fi

    # Extract seed - pattern: ..., SEED)
    if [[ "$line" =~ ,\ *([0-9]+)\ *\) ]]; then
        seed="${BASH_REMATCH[1]}"
        location="${line%%:*}:${line#*:}"
        location="${location%%:*}"  # file:line

        if [ -n "${seed_map[$seed]:-}" ]; then
            echo -e "${RED}ERROR: Duplicate seed $seed found:${NC}"
            echo "  - ${seed_map[$seed]}"
            echo "  - $location"
            duplicates_found=1
        else
            seed_map[$seed]="$location"
        fi
    fi
done <<< "$all_seeds"

# Also check for AuraEffectSystem::testing() - these should be converted to simulation
testing_calls=$(grep -rn "AuraEffectSystem::testing" crates --include="*.rs" | grep -E "(tests/|test\.rs|_test\.rs)" || true)

if [ -n "$testing_calls" ]; then
    echo -e "${YELLOW}WARNING: Found tests using AuraEffectSystem::testing() without unique seeds:${NC}"
    echo "$testing_calls" | head -10
    if [ $(echo "$testing_calls" | wc -l) -gt 10 ]; then
        echo "  ... and $(($(echo "$testing_calls" | wc -l) - 10)) more"
    fi
    echo ""
    echo "Consider using AuraEffectSystem::simulation(&config, UNIQUE_SEED) instead"
    echo "to ensure test isolation and avoid encryption key caching issues."
fi

if [ $duplicates_found -eq 1 ]; then
    echo -e "${RED}Found duplicate seed(s)${NC}"
    echo ""
    echo "Each test should use a unique deterministic seed to ensure proper isolation."
    echo "Recommended pattern:"
    echo "  let effects = Arc::new(AuraEffectSystem::simulation(&config, UNIQUE_SEED).unwrap());"
    echo ""
    echo "Where UNIQUE_SEED is a number unique to that test (e.g., 10001, 10002, ...)."
    exit 1
fi

echo -e "${GREEN}✓ All test seeds are unique (${#seed_map[@]} unique seeds found)${NC}"
exit 0
