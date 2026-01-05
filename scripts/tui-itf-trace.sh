#!/usr/bin/env bash
# tui-itf-trace.sh - Generate or check TUI ITF traces
#
# Usage:
#   ./scripts/tui-itf-trace.sh generate [output]   # Generate trace (default)
#   ./scripts/tui-itf-trace.sh check [expected]    # Check trace matches regeneration
#
# Environment:
#   TUI_ITF_SEED       - Random seed (default: 424242)
#   TUI_ITF_MAX_STEPS  - Max simulation steps (default: 50)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPEC="$ROOT/verification/quint/tui_state_machine.qnt"
DEFAULT_TRACE="$ROOT/verification/quint/traces/tui_trace.itf.json"

SEED="${TUI_ITF_SEED:-424242}"
MAX_STEPS="${TUI_ITF_MAX_STEPS:-50}"

# ============================================================================
# Helpers
# ============================================================================

generate_trace() {
    local out="$1"

    if ! command -v quint >/dev/null 2>&1; then
        echo "error: quint not found in PATH (run inside \`nix develop\`)" >&2
        exit 1
    fi

    mkdir -p "$(dirname "$out")"

    local raw
    raw="$(mktemp)"
    trap "rm -f '$raw'" EXIT

    quint run \
        --seed="$SEED" \
        --max-samples=1 \
        --n-traces=1 \
        --max-steps="$MAX_STEPS" \
        --invariants allInvariants \
        --out-itf="$raw" \
        "$SPEC" \
        >/dev/null

    # Strip volatile metadata for stable diffs (jq is available in nix develop)
    if command -v jq >/dev/null 2>&1; then
        jq -c 'del(."#meta".timestamp, ."#meta".description)' "$raw" >"$out"
    else
        # Fallback: copy without stripping metadata (may cause spurious diffs)
        mv "$raw" "$out"
    fi

    echo "wrote $out (seed=$SEED, max_steps=$MAX_STEPS)"
}

check_trace() {
    local expected="$1"

    if [ ! -f "$expected" ]; then
        echo "error: expected trace not found at $expected" >&2
        exit 1
    fi

    local tmp
    tmp="$(mktemp)"
    trap "rm -f '$tmp'" EXIT

    # Generate to temp file (suppress output)
    generate_trace "$tmp" >/dev/null

    if cmp -s "$expected" "$tmp"; then
        echo "[OK] ITF trace is up to date (${expected#$ROOT/})"
        exit 0
    fi

    echo "[FAIL] ITF trace differs from regenerated output: ${expected#$ROOT/}" >&2
    echo "" >&2

    if command -v git >/dev/null 2>&1; then
        git diff --no-index -- "$expected" "$tmp" || true
    else
        diff -u "$expected" "$tmp" || true
    fi

    echo "" >&2
    echo "To update: just tui-itf-trace" >&2
    exit 1
}

# ============================================================================
# Main
# ============================================================================

CMD="${1:-generate}"
shift || true

case "$CMD" in
    generate|gen)
        OUT="${1:-$DEFAULT_TRACE}"
        generate_trace "$OUT"
        ;;
    check)
        EXPECTED="${1:-$DEFAULT_TRACE}"
        check_trace "$EXPECTED"
        ;;
    *)
        echo "Usage: $0 {generate|check} [path]" >&2
        exit 1
        ;;
esac
