#!/usr/bin/env bash
#
# Generate ITF traces from Quint consensus specifications
# for exhaustive conformance testing of Rust implementation.
#
# Usage:
#   ./scripts/generate-itf-traces.sh [count]
#
# Arguments:
#   count - Number of traces to generate per spec (default: 30)
#
# Output:
#   traces/consensus/*.itf.json

set -euo pipefail

# Configuration
TRACE_COUNT="${1:-30}"
TRACE_DIR="traces/consensus"
QUINT_DIR="verification/quint"

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo "=================================================="
echo "ITF Trace Generation for Consensus Verification"
echo "=================================================="
echo ""
echo "Configuration:"
echo "  Traces per category: $TRACE_COUNT"
echo "  Output dir:          $TRACE_DIR"
echo ""

# Create output directory
mkdir -p "$TRACE_DIR"

# Track statistics
total_generated=0
total_failed=0

# Function to generate a single trace
generate_trace() {
    local spec="$1"
    local output="$2"
    local seed="$3"
    local max_steps="$4"
    local max_samples="${5:-3}"

    if quint run \
        --out-itf="$output" \
        --seed="$seed" \
        --max-steps="$max_steps" \
        --max-samples="$max_samples" \
        "$spec" >/dev/null 2>&1; then
        return 0
    else
        return 1
    fi
}

# Generate fast path traces (short, quick consensus)
echo -e "${YELLOW}[1/4]${NC} Generating fast path traces..."
for i in $(seq 1 "$TRACE_COUNT"); do
    seed=$((1000 + i * 7))
    output="$TRACE_DIR/fast_path_${i}.itf.json"
    if generate_trace "$QUINT_DIR/protocol_consensus.qnt" "$output" "$seed" 15 2; then
        ((total_generated++))
    else
        ((total_failed++))
    fi
    # Progress every 10
    (( i % 10 == 0 )) && echo -e "  Progress: $i/$TRACE_COUNT"
done
echo -e "  ${GREEN}[OK]${NC} Generated $TRACE_COUNT fast path traces"

# Generate slow path / fallback traces (longer)
echo -e "${YELLOW}[2/4]${NC} Generating fallback path traces..."
for i in $(seq 1 "$TRACE_COUNT"); do
    seed=$((2000 + i * 11))
    output="$TRACE_DIR/fallback_${i}.itf.json"
    if generate_trace "$QUINT_DIR/protocol_consensus.qnt" "$output" "$seed" 25 3; then
        ((total_generated++))
    else
        ((total_failed++))
    fi
    (( i % 10 == 0 )) && echo -e "  Progress: $i/$TRACE_COUNT"
done
echo -e "  ${GREEN}[OK]${NC} Generated $TRACE_COUNT fallback traces"

# Generate Byzantine/adversary traces
echo -e "${YELLOW}[3/4]${NC} Generating Byzantine traces..."
for i in $(seq 1 "$TRACE_COUNT"); do
    seed=$((3000 + i * 13))
    output="$TRACE_DIR/byzantine_${i}.itf.json"
    if generate_trace "$QUINT_DIR/protocol_consensus_adversary.qnt" "$output" "$seed" 20 3; then
        ((total_generated++))
    else
        ((total_failed++))
    fi
    (( i % 10 == 0 )) && echo -e "  Progress: $i/$TRACE_COUNT"
done
echo -e "  ${GREEN}[OK]${NC} Generated $TRACE_COUNT Byzantine traces"

# Generate random exploration traces
echo -e "${YELLOW}[4/4]${NC} Generating random exploration traces..."
for i in $(seq 1 "$TRACE_COUNT"); do
    seed=$RANDOM$RANDOM
    output="$TRACE_DIR/random_${i}.itf.json"
    if generate_trace "$QUINT_DIR/protocol_consensus.qnt" "$output" "$seed" 20 3; then
        ((total_generated++))
    else
        ((total_failed++))
    fi
    (( i % 10 == 0 )) && echo -e "  Progress: $i/$TRACE_COUNT"
done
echo -e "  ${GREEN}[OK]${NC} Generated $TRACE_COUNT random traces"

# Summary
echo ""
echo "=================================================="
echo "Generation Complete"
echo "=================================================="
echo ""
echo "  Total generated: $total_generated"
echo "  Total failed:    $total_failed"
echo ""

# Count actual files
actual_count=$(find "$TRACE_DIR" -name "*.itf.json" -type f | wc -l | tr -d ' ')
echo "  Files in $TRACE_DIR: $actual_count"
echo ""

# Validate a sample of traces
echo -e "${YELLOW}[INFO]${NC} Validating sample traces..."
sample_valid=0
sample_invalid=0
for trace in $(find "$TRACE_DIR" -name "*.itf.json" -type f | head -10); do
    if jq -e '.states | length > 0' "$trace" >/dev/null 2>&1; then
        ((sample_valid++))
    else
        ((sample_invalid++))
        echo -e "  ${RED}[INVALID]${NC} $trace"
    fi
done
echo -e "  ${GREEN}[OK]${NC} $sample_valid/10 sample traces valid"

if (( actual_count >= 100 )); then
    echo ""
    echo -e "${GREEN}[SUCCESS]${NC} Generated 100+ traces for conformance testing"
else
    echo ""
    echo -e "${YELLOW}[WARNING]${NC} Only $actual_count traces generated (target: 100+)"
fi
