#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

EXPECTED_REL="${1:-verification/quint/tui_trace.itf.json}"
EXPECTED="$ROOT/$EXPECTED_REL"

if [ ! -f "$EXPECTED" ]; then
  echo "error: expected trace not found at $EXPECTED_REL" >&2
  exit 1
fi

TMP="$(mktemp)"
cleanup() { rm -f "$TMP"; }
trap cleanup EXIT

"$ROOT/scripts/gen-tui-itf-trace.sh" "$TMP" >/dev/null

if cmp -s "$EXPECTED" "$TMP"; then
  echo "[OK] ITF trace is up to date ($EXPECTED_REL)"
  exit 0
fi

echo "[FAIL] ITF trace differs from regenerated output: $EXPECTED_REL" >&2
echo "" >&2

if command -v git >/dev/null 2>&1; then
  git diff --no-index -- "$EXPECTED" "$TMP" || true
else
  diff -u "$EXPECTED" "$TMP" || true
fi

echo "" >&2
echo "To update:" >&2
echo "  just tui-itf-trace" >&2
exit 1
