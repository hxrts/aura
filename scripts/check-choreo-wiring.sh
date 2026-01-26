#!/usr/bin/env bash
set -euo pipefail

DOC_PATH="docs/107_mpst_and_choreography.md"

if [[ ! -f "$DOC_PATH" ]]; then
  echo "Missing choreography audit doc: $DOC_PATH"
  exit 1
fi

spec_only_protocols=$(rg -n "\\| Spec-only \\|" "$DOC_PATH" \
  | awk -F'|' '{gsub(/^[ \t]+|[ \t]+$/, "", $2); print $2}')

if [[ -z "$spec_only_protocols" ]]; then
  echo "No Spec-only protocols listed; skipping check."
  exit 0
fi

violations=0
for protocol in $spec_only_protocols; do
  if rg -n "\\b${protocol}\\b" crates/aura-agent crates/aura-app crates/aura-terminal >/dev/null 2>&1; then
    echo "::error::Protocol '$protocol' is marked Spec-only but referenced in runtime/app/UI code."
    violations=1
  fi
done

if [[ $violations -ne 0 ]]; then
  echo "Choreography wiring lint failed."
  exit 1
fi

echo "Choreography wiring lint passed."
