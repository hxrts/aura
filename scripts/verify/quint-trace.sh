#!/usr/bin/env bash
# quint-trace.sh - Generate or check deterministic Quint semantic traces
#
# Usage:
#   ./scripts/verify/quint-trace.sh generate [spec] [output]
#   ./scripts/verify/quint-trace.sh check [spec] [expected]
#
# Environment:
#   QUINT_TRACE_SEED       - Random seed (default: 424242)
#   QUINT_TRACE_MAX_STEPS  - Max simulation steps (default: 50)
#   QUINT_TRACE_MAIN       - Optional --main override

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEFAULT_SPEC="$ROOT/verification/quint/harness/flows.qnt"
DEFAULT_TRACE="$ROOT/verification/quint/traces/harness_flows.itf.json"

SEED="${QUINT_TRACE_SEED:-424242}"
MAX_STEPS="${QUINT_TRACE_MAX_STEPS:-50}"
MAIN="${QUINT_TRACE_MAIN:-}"
if [[ -z "$MAIN" && "$(basename "$DEFAULT_SPEC")" == "flows.qnt" ]]; then
    MAIN="fullInvitationChatScenario"
fi

generate_trace() {
    local spec="$1"
    local out="$2"

    if ! command -v quint >/dev/null 2>&1; then
        echo "error: quint not found in PATH (run inside \`nix develop\`)" >&2
        exit 1
    fi

    mkdir -p "$(dirname "$out")"

    local raw
    raw="$(mktemp)"
    trap "rm -f '$raw'" EXIT

    local args=(
        run
        "--seed=$SEED"
        "--max-samples=1"
        "--n-traces=1"
        "--max-steps=$MAX_STEPS"
        "--out-itf=$raw"
    )
    if [[ -n "$MAIN" ]]; then
        args+=("--main=$MAIN")
    fi
    args+=("$spec")

    quint "${args[@]}" >/dev/null

    if command -v jq >/dev/null 2>&1; then
        jq -c 'del(."#meta".timestamp, ."#meta".description)' "$raw" >"$out"
    else
        mv "$raw" "$out"
    fi

    echo "wrote $out from ${spec#$ROOT/} (seed=$SEED, max_steps=$MAX_STEPS)"
}

check_trace() {
    local spec="$1"
    local expected="$2"

    if [ ! -f "$expected" ]; then
        echo "error: expected trace not found at $expected" >&2
        exit 1
    fi

    local tmp
    tmp="$(mktemp)"
    trap "rm -f '$tmp'" EXIT

    generate_trace "$spec" "$tmp" >/dev/null

    if cmp -s "$expected" "$tmp"; then
        echo "[OK] Quint semantic trace is up to date (${expected#$ROOT/})"
        exit 0
    fi

    echo "[FAIL] Quint semantic trace differs from regenerated output: ${expected#$ROOT/}" >&2
    echo "" >&2

    if command -v git >/dev/null 2>&1; then
        git diff --no-index -- "$expected" "$tmp" || true
    else
        diff -u "$expected" "$tmp" || true
    fi

    echo "" >&2
    echo "To update: just quint-semantic-trace" >&2
    exit 1
}

CMD="${1:-generate}"
SPEC_INPUT="${2:-$DEFAULT_SPEC}"
PATH_INPUT="${3:-$DEFAULT_TRACE}"

case "$CMD" in
    generate|gen)
        generate_trace "$SPEC_INPUT" "$PATH_INPUT"
        ;;
    check)
        check_trace "$SPEC_INPUT" "$PATH_INPUT"
        ;;
    *)
        echo "Usage: $0 {generate|check} [spec] [path]" >&2
        exit 1
        ;;
esac
