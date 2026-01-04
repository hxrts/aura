#!/usr/bin/env bash
# check-quint-rust-types.sh - Detect type drift between Quint specs and Rust implementations
#
# This script extracts type definitions from Quint specifications and compares them
# against QuintMappable implementations in Rust to catch drift early.
#
# Usage: ./scripts/check-quint-rust-types.sh [--verbose]

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VERBOSE="${1:-}"

QUINT_DIR="$PROJECT_ROOT/verification/quint"
RUST_SRC="$PROJECT_ROOT/crates"
TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Main logic
log_info "Quint-Rust Type Drift Detection"
log_info "==============================="
echo

# Step 1: Extract Quint types
log_info "Extracting Quint type definitions..."
: > "$TEMP_DIR/quint_types.txt"

for qnt_file in $(find "$QUINT_DIR" -name "*.qnt" -type f); do
    rel_path="${qnt_file#$QUINT_DIR/}"

    # Extract type definitions
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

# Step 2: Extract Rust QuintMappable impls
log_info "Extracting Rust QuintMappable implementations..."
: > "$TEMP_DIR/rust_mappables.txt"

grep -rn "impl QuintMappable for" "$RUST_SRC" --include="*.rs" 2>/dev/null | while IFS=: read -r file_path line_num impl_line; do
    type_name=$(echo "$impl_line" | sed -E 's/.*impl[[:space:]]+QuintMappable[[:space:]]+for[[:space:]]+([A-Za-z0-9_]+).*/\1/')
    rel_path="${file_path#$RUST_SRC/}"

    if [[ -n "$type_name" && ! "$type_name" =~ \{ ]]; then
        # Extract quint_type_name()
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

# Step 3: Compare types
log_info "Comparing Quint and Rust types..."

# Build lookup of Rust types
declare -A rust_by_quint
while IFS=: read -r _ rust_type quint_name path line; do
    if [[ -n "$quint_name" ]]; then
        rust_by_quint["$quint_name"]="$rust_type|$path:$line"
    fi
done < "$TEMP_DIR/rust_mappables.txt"

matched=0
unmapped=0

while IFS=: read -r kind quint_type qnt_path qnt_line; do
    # Skip helper types
    case "$quint_type" in
        Option|DataBinding|CachedNonce|NonceCommitment|OperationData|Epoch)
            continue ;;
    esac

    if [[ -n "${rust_by_quint[$quint_type]:-}" ]]; then
        if [[ "$VERBOSE" == "--verbose" ]]; then
            echo "  ✓ $quint_type -> ${rust_by_quint[$quint_type]}"
        fi
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

# Verbose report
if [[ "$VERBOSE" == "--verbose" ]]; then
    echo
    echo "=== Mapped Types ==="
    while IFS=: read -r kind quint_type qnt_path qnt_line; do
        case "$quint_type" in
            Option|DataBinding|CachedNonce|NonceCommitment|OperationData|Epoch) continue ;;
        esac
        if [[ -n "${rust_by_quint[$quint_type]:-}" ]]; then
            info="${rust_by_quint[$quint_type]}"
            rust_type="${info%%|*}"
            rust_loc="${info#*|}"
            echo "$quint_type ($qnt_path:$qnt_line) -> $rust_type ($rust_loc)"
        fi
    done < "$TEMP_DIR/quint_types.txt"

    echo
    echo "=== Unmapped Quint Types ==="
    while IFS=: read -r kind quint_type qnt_path qnt_line; do
        case "$quint_type" in
            Option|DataBinding|CachedNonce|NonceCommitment|OperationData|Epoch) continue ;;
        esac
        if [[ -z "${rust_by_quint[$quint_type]:-}" ]]; then
            echo "- $quint_type ($kind) - $qnt_path:$qnt_line"
        fi
    done < "$TEMP_DIR/quint_types.txt"
fi

echo
log_info "✓ Type drift check complete"
