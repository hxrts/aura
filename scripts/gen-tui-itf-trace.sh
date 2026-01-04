#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

OUT="${1:-$ROOT/verification/quint/traces/tui_trace.itf.json}"
SEED="${TUI_ITF_SEED:-424242}"
MAX_STEPS="${TUI_ITF_MAX_STEPS:-50}"

SPEC="$ROOT/verification/quint/tui_state_machine.qnt"

if ! command -v quint >/dev/null 2>&1; then
  echo "error: quint not found in PATH (run inside \`nix develop\`)" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUT")"

RAW="$(mktemp)"
cleanup() { rm -f "$RAW"; }
trap cleanup EXIT

quint run \
  --seed="$SEED" \
  --max-samples=1 \
  --n-traces=1 \
  --max-steps="$MAX_STEPS" \
  --invariants allInvariants \
  --out-itf="$RAW" \
  "$SPEC" \
  >/dev/null

if command -v jq >/dev/null 2>&1; then
  # Quint includes wall-clock metadata that changes on every run.
  # Strip it so the checked-in trace is stable and easy to compare in CI.
  jq -c 'del(."#meta".timestamp, ."#meta".description)' "$RAW" >"$OUT"
elif command -v python3 >/dev/null 2>&1; then
  python3 - "$RAW" "$OUT" <<'PY'
import json
import sys

raw_path, out_path = sys.argv[1], sys.argv[2]
with open(raw_path, "r", encoding="utf-8") as f:
    data = json.load(f)
meta = data.get("#meta", {})
meta.pop("timestamp", None)
meta.pop("description", None)
data["#meta"] = meta
with open(out_path, "w", encoding="utf-8") as f:
    json.dump(data, f, separators=(",", ":"))
    f.write("\n")
PY
else
  mv "$RAW" "$OUT"
fi

echo "wrote $OUT (seed=$SEED, max_steps=$MAX_STEPS)"
