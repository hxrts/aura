#!/usr/bin/env bash
# Check for emojis in non-gitignored project files
#
# Usage:
#   ./scripts/check/docs-text-formatting.sh           # Check only (for CI)
#   ./scripts/check/docs-text-formatting.sh --replace # Check and auto-fix
#
# Auto-replacements (with --replace):
#   ✅ (green check) → ✓
#   ❌ (red x) → ✗
#   ⚠️ (warning emoji) → ⚠ (strips variation selector)
#
# Allowed symbols (not emojis):
#   ✓ ✗ ✔ ✕ ✖ ➤ ⚠ ⚙ ⚖ ✉ ★ ☆ ♥ and other dingbats/symbols
#
# Reports emojis with file locations and line numbers

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Parse arguments
REPLACE_MODE=false
if [[ "${1:-}" == "--replace" ]]; then
    REPLACE_MODE=true
fi

# Detect sed in-place flag (GNU sed vs BSD sed)
if sed --version 2>/dev/null | grep -q "GNU"; then
    SED_INPLACE="sed -i"
else
    SED_INPLACE="sed -i ''"
fi

found_any=false

echo "Scanning for emojis in tracked files..."
echo

if [[ "$REPLACE_MODE" == true ]]; then
    # Replace mode: auto-fix known emoji → symbol replacements
    check_count=0
    x_count=0
    warning_count=0

    while IFS= read -r file; do
        # Skip this script itself (contains emoji documentation)
        if [[ "$file" == "scripts/check/docs-text-formatting.sh" ]]; then
            continue
        fi

        # Skip binary files and non-existent files
        if [[ ! -f "$file" ]] || file "$file" | grep -q "binary"; then
            continue
        fi

        # Check for green check emoji and replace
        if grep -q '✅' "$file" 2>/dev/null; then
            count=$(grep -o '✅' "$file" | wc -l | tr -d ' ')
            check_count=$((check_count + count))
            $SED_INPLACE 's/✅/✓/g' "$file"
            echo -e "${GREEN}Fixed${NC} $file: replaced $count ✅ → ✓"
        fi

        # Check for red x emoji and replace
        if grep -q '❌' "$file" 2>/dev/null; then
            count=$(grep -o '❌' "$file" | wc -l | tr -d ' ')
            x_count=$((x_count + count))
            $SED_INPLACE 's/❌/✗/g' "$file"
            echo -e "${GREEN}Fixed${NC} $file: replaced $count ❌ → ✗"
        fi

        # Strip variation selector from warning symbol (⚠️ → ⚠)
        # U+26A0 followed by U+FE0F (variation selector-16)
        if grep -qP '⚠\x{FE0F}' "$file" 2>/dev/null; then
            count=$(grep -oP '⚠\x{FE0F}' "$file" | wc -l | tr -d ' ')
            warning_count=$((warning_count + count))
            $SED_INPLACE $'s/⚠\xef\xb8\x8f/⚠/g' "$file"
            echo -e "${GREEN}Fixed${NC} $file: replaced $count ⚠️ → ⚠"
        fi
    done < <(git ls-files)

    if [[ $check_count -gt 0 ]] || [[ $x_count -gt 0 ]] || [[ $warning_count -gt 0 ]]; then
        echo
        echo -e "${GREEN}Auto-fixed:${NC} $check_count ✅→✓, $x_count ❌→✗, $warning_count ⚠️→⚠"
        echo
    fi

    echo "Searching for remaining emojis..."
    echo
fi

# Check for all emojis (including auto-fixable ones in check-only mode)
while IFS= read -r file; do
    # Skip this script itself (contains emoji documentation)
    if [[ "$file" == "scripts/check/docs-text-formatting.sh" ]]; then
        continue
    fi

    # Skip binary files and non-existent files
    if [[ ! -f "$file" ]] || file "$file" | grep -q "binary"; then
        continue
    fi

    # Search for emoji ranges:
    # U+1F300-U+1F9FF: Miscellaneous Symbols and Pictographs, Emoticons, etc.
    # U+1FA00-U+1FAFF: Symbols and Pictographs Extended-A
    # Also check for auto-fixable emojis: ✅ (U+2705), ❌ (U+274C), ⚠️ (U+26A0+FE0F)
    if matches=$(grep -n -P '[\x{1F300}-\x{1F9FF}]|[\x{1FA00}-\x{1FAFF}]|✅|❌|⚠\x{FE0F}' "$file" 2>/dev/null); then
        found_any=true
        echo -e "${YELLOW}$file${NC}"
        echo "$matches" | while IFS= read -r line; do
            echo "  $line"
        done
        echo
    fi
done < <(git ls-files)

if [[ "$found_any" == false ]]; then
    echo -e "${GREEN}No emojis found.${NC}"
else
    echo -e "${RED}Found emojis in the files listed above.${NC}"
    if [[ "$REPLACE_MODE" == false ]]; then
        echo -e "Run with ${YELLOW}--replace${NC} to auto-fix ✅→✓, ❌→✗, ⚠️→⚠"
    fi
    exit 1
fi
