#!/usr/bin/env bash
# Canonical complete consensus proof gate; called inside the tracked build budget.
set -euo pipefail
export NO_COLOR=1
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
expected="$here/kani-consensus-inventory.txt"
artifacts=${AURA_KANI_ARTIFACTS:-artifacts/kani}
mkdir -p "$artifacts"
[[ $(wc -l < "$expected" | tr -d ' ') == 134 ]]
[[ $(sort -u "$expected" | wc -l | tr -d ' ') == 134 ]]
# Refuse an equal-sized but incomplete/duplicated Cartesian refinement family.
for phase in 0 1 2 3 4; do
  for transition in 0 1 2; do
    for binding in 0 1 2 3 4 5 6 7; do
      printf 'core::verification::kani_proofs::production_wrappers_refine_bounded_decisions_p%s_t%s_b%s\n' "$phase" "$transition" "$binding"
    done
  done
done | sort > "$artifacts/required-family.txt"
rg '::production_wrappers_' "$expected" > "$artifacts/registered-family.txt"
diff -u "$artifacts/required-family.txt" "$artifacts/registered-family.txt"
# Kani 0.68.0 writes authoritative JSON to this fixed cwd path. Never certify
# stale output; retain the exact producer version/schema with the gate artifacts.
[[ ! -e kani-list.json ]] || { echo 'Refusing existing kani-list.json discovery output' >&2; exit 2; }
just _run-kani cargo kani list --package hxrts-aura-consensus --format json > "$artifacts/discovery.log" 2>&1
[[ -f kani-list.json ]] || { echo 'Kani did not produce discovery JSON' >&2; exit 2; }
mv kani-list.json "$artifacts/discovery.json"
jq -e '
  .["kani-version"] == "0.68.0" and .["file-version"] == "0.1" and
  (.["standard-harnesses"] | type == "object") and
  (.["standard-harnesses"] | all(.[]; type == "array" and all(.[]; type == "string"))) and
  .totals["standard-harnesses"] == 134 and
  .totals["contract-harnesses"] == 0 and
  .totals["functions-under-contract"] == 0 and
  (.["contract-harnesses"] | length == 0) and
  (.contracts | length == 0)
' "$artifacts/discovery.json" > /dev/null
jq -r '.["standard-harnesses"][][]' "$artifacts/discovery.json" | sort > "$artifacts/discovered.txt"
diff -u "$expected" "$artifacts/discovered.txt"
status=0
just _run-kani cargo kani --package hxrts-aura-consensus --default-unwind 10 --output-format regular > "$artifacts/proofs.log" 2>&1 || status=$?
cat "$artifacts/proofs.log"
(( status == 0 )) || exit "$status"
# A successful empty/partial invocation cannot certify the complete inventory.
sed -n 's/^Thread [0-9][0-9]*: //;s/^Checking harness \(core::verification::kani_proofs::[A-Za-z0-9_]*\)\.\.\.$/\1/p' "$artifacts/proofs.log" | sort > "$artifacts/executed.txt"
diff -u "$expected" "$artifacts/executed.txt"
