#!/usr/bin/env bash
# Validate docs/998_verification_coverage.md metrics against actual codebase counts.
#
# This script parses the verification coverage document and compares each
# documented metric against the actual count in the codebase.
#
# Usage:
#   ./scripts/check-verification-coverage.sh
#
# Exit codes:
#   0 - All metrics match
#   1 - One or more metrics differ
#   2 - Script error (missing dependencies, files not found)

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

DOC="docs/998_verification_coverage.md"
if [[ ! -f "$DOC" ]]; then
  echo "error: $DOC not found" >&2
  exit 2
fi

# Colors (disable if not a TTY for CI compatibility)
if [[ -t 1 ]]; then
  RED='\033[0;31m'
  GREEN='\033[0;32m'
  YELLOW='\033[0;33m'
  NC='\033[0m'
else
  RED=''
  GREEN=''
  YELLOW=''
  NC=''
fi

mismatches=0
checks_run=0

# Helper to compare documented vs actual count
check_metric() {
  local name="$1"
  local documented="$2"
  local actual="$3"

  checks_run=$((checks_run + 1))

  if [[ -z "$documented" ]]; then
    printf "  ${YELLOW}?${NC} %-30s %4d (not documented)\n" "$name" "$actual"
    mismatches=$((mismatches + 1))
  elif [[ "$documented" -eq "$actual" ]]; then
    printf "  ${GREEN}✓${NC} %-30s %4d\n" "$name" "$actual"
  else
    printf "  ${RED}✗${NC} %-30s %4d (doc: %d, diff: %+d)\n" "$name" "$actual" "$documented" "$((actual - documented))"
    mismatches=$((mismatches + 1))
  fi
}

# Extract documented value from the summary table (| Metric | Count |)
get_documented() {
  local metric="$1"
  grep -E "^\| ${metric} \|" "$DOC" 2>/dev/null | sed -E 's/.*\| ([0-9]+) \|.*/\1/' | head -1
}

# Extract count from prose like "17 modules" or "(10 files)"
get_prose_count() {
  local pattern="$1"
  grep -oE "$pattern" "$DOC" 2>/dev/null | grep -oE '[0-9]+' | head -1
}

# Extract invariant entries from tables: | `InvariantName` | file.qnt |
get_listed_invariants() {
  grep -E '^\| `Invariant[^`]+` \|' "$DOC" 2>/dev/null | \
    sed -E 's/^\| `([^`]+)` \| ([^ |]+).*/\1:\2/' | \
    tr -d ' '
}

# Extract temporal property entries from tables: | `propertyName` | file.qnt |
get_listed_temporal() {
  # Match the Temporal Properties section (skip header line to avoid immediate termination)
  awk '/^### Temporal Properties/{found=1; next} found && /^###|^## /{exit} found' "$DOC" 2>/dev/null | \
    grep -E '^\| `[^`]+` \|' | \
    sed -E 's/^\| `([^`]+)` \| ([^ |]+).*/\1:\2/' | \
    tr -d ' '
}

echo "Verification Coverage Check"
echo "============================"
echo ""
echo "Summary Metrics"
echo "---------------"

# ─────────────────────────────────────────────────────────────────────────────
# Summary table metrics
# ─────────────────────────────────────────────────────────────────────────────

quint_specs=$(find verification/quint -name "*.qnt" -type f 2>/dev/null | wc -l | tr -d ' ')
check_metric "Quint Specifications" "$(get_documented "Quint Specifications")" "$quint_specs"

quint_invariants=$(grep -rhE "^\s*val [A-Za-z]*[Ii]nvariant" verification/quint/ 2>/dev/null | wc -l | tr -d ' ')
check_metric "Quint Invariants" "$(get_documented "Quint Invariants")" "$quint_invariants"

quint_temporal=$(grep -rhE "^\s*temporal [a-zA-Z]" verification/quint/ 2>/dev/null | wc -l | tr -d ' ')
check_metric "Quint Temporal Properties" "$(get_documented "Quint Temporal Properties")" "$quint_temporal"

quint_types=$(grep -rhE "^\s*type " verification/quint/ 2>/dev/null | wc -l | tr -d ' ')
check_metric "Quint Type Definitions" "$(get_documented "Quint Type Definitions")" "$quint_types"

lean_files=$(find verification/lean -name "*.lean" -type f 2>/dev/null | wc -l | tr -d ' ')
check_metric "Lean Source Files" "$(get_documented "Lean Source Files")" "$lean_files"

lean_theorems=$(grep -rhE "^(theorem|lemma) " verification/lean/ 2>/dev/null | wc -l | tr -d ' ')
check_metric "Lean Theorems" "$(get_documented "Lean Theorems")" "$lean_theorems"

conformance_fixtures=$(find crates/aura-testkit/tests/fixtures/conformance -name "*.json" -type f 2>/dev/null | wc -l | tr -d ' ')
check_metric "Conformance Fixtures" "$(get_documented "Conformance Fixtures")" "$conformance_fixtures"

itf_harnesses=$(find verification/quint/harness -name "*.qnt" -type f 2>/dev/null | wc -l | tr -d ' ')
check_metric "ITF Trace Harnesses" "$(get_documented "ITF Trace Harnesses")" "$itf_harnesses"

testkit_tests=$(grep -rh "^#\[test\]" crates/aura-testkit/src/ crates/aura-testkit/tests/ 2>/dev/null | wc -l | tr -d ' ')
check_metric "Testkit Tests" "$(get_documented "Testkit Tests")" "$testkit_tests"

bridge_modules=$(find crates/aura-quint/src -name "bridge_*.rs" -type f 2>/dev/null | wc -l | tr -d ' ')
check_metric "Bridge Modules" "$(get_documented "Bridge Modules")" "$bridge_modules"

# Count CI verification gates from justfile
ci_gates=$(grep -cE "^ci-(property-monitor|choreo-parity|quint-typecheck|conformance-policy|conformance-contracts|lean-build|lean-check-sorry|lean-quint-bridge|kani):" justfile 2>/dev/null || echo "0")
# Add 1 for conformance_golden_fixtures test
ci_gates=$((ci_gates + 1))
check_metric "CI Verification Gates" "$(get_documented "CI Verification Gates")" "$ci_gates"

echo ""
echo "Quint Subsystem Breakdown"
echo "-------------------------"

# ─────────────────────────────────────────────────────────────────────────────
# Quint subsystem file counts (from | Subsystem | Files | table)
# ─────────────────────────────────────────────────────────────────────────────

check_subsystem() {
  local name="$1"
  local path="$2"
  local actual
  actual=$(find "$path" -maxdepth 1 -name "*.qnt" -type f 2>/dev/null | wc -l | tr -d ' ')
  local documented
  documented=$(grep -E "^\| ${name} \|" "$DOC" 2>/dev/null | sed -E 's/.*\| ([0-9]+) \|.*/\1/' | head -1)
  check_metric "$name" "$documented" "$actual"
}

check_subsystem "Root" "verification/quint"
check_subsystem "consensus/" "verification/quint/consensus"
check_subsystem "journal/" "verification/quint/journal"
check_subsystem "keys/" "verification/quint/keys"
check_subsystem "sessions/" "verification/quint/sessions"
check_subsystem "amp/" "verification/quint/amp"
check_subsystem "liveness/" "verification/quint/liveness"
check_subsystem "harness/" "verification/quint/harness"
check_subsystem "tui/" "verification/quint/tui"

echo ""
echo "Lean Module Breakdown"
echo "---------------------"

# ─────────────────────────────────────────────────────────────────────────────
# Lean module counts - extract from prose like "(10 files)" or "(14 files,"
# ─────────────────────────────────────────────────────────────────────────────

lean_type_modules=$(find verification/lean/Aura/Types -name "*.lean" -type f 2>/dev/null | wc -l | tr -d ' ')
lean_type_modules=$((lean_type_modules + 1))  # Add Types.lean itself
doc_type_modules=$(get_prose_count "Type Modules \([0-9]+ files\)")
check_metric "Lean Type Modules" "$doc_type_modules" "$lean_type_modules"

lean_domain_modules=$(find verification/lean/Aura/Domain -name "*.lean" -type f 2>/dev/null | wc -l | tr -d ' ')
doc_domain_modules=$(get_prose_count "Domain Modules \([0-9]+ files\)")
check_metric "Lean Domain Modules" "$doc_domain_modules" "$lean_domain_modules"

lean_proof_modules=$(find verification/lean/Aura/Proofs -name "*.lean" -type f 2>/dev/null | wc -l | tr -d ' ')
doc_proof_modules=$(get_prose_count "Proof Modules \([0-9]+ files")
check_metric "Lean Proof Modules" "$doc_proof_modules" "$lean_proof_modules"

lean_entry_points=0
for f in verification/lean/Aura.lean verification/lean/Aura/Proofs.lean verification/lean/Aura/Assumptions.lean verification/lean/Aura/Runner.lean; do
  [[ -f "$f" ]] && lean_entry_points=$((lean_entry_points + 1))
done
doc_entry_points=$(get_prose_count "Entry Points \([0-9]+ files\)")
check_metric "Lean Entry Points" "$doc_entry_points" "$lean_entry_points"

echo ""
echo "Simulator Integration"
echo "---------------------"

# ─────────────────────────────────────────────────────────────────────────────
# Simulator quint module count - extract from "17 modules implementing"
# ─────────────────────────────────────────────────────────────────────────────

sim_modules=$(find crates/aura-simulator/src/quint -name "*.rs" -type f 2>/dev/null | wc -l | tr -d ' ')
doc_sim_modules=$(get_prose_count "[0-9]+ modules implementing generative simulation")
check_metric "Simulator Quint Modules" "$doc_sim_modules" "$sim_modules"

# Check differential tester exists (mentioned in doc)
checks_run=$((checks_run + 1))
if [[ -f "crates/aura-simulator/src/differential_tester.rs" ]]; then
  printf "  ${GREEN}✓${NC} Differential tester exists\n"
else
  printf "  ${RED}✗${NC} Differential tester missing\n"
  mismatches=$((mismatches + 1))
fi

echo ""
echo "Bridge Verification"
echo "-------------------"

# Check bridge modules exist
bridge_ok=0
bridge_missing=0
for mod in bridge_export bridge_import bridge_format bridge_validate; do
  if [[ -f "crates/aura-quint/src/${mod}.rs" ]]; then
    bridge_ok=$((bridge_ok + 1))
  else
    echo "  missing: ${mod}.rs"
    bridge_missing=$((bridge_missing + 1))
  fi
done

checks_run=$((checks_run + 1))
if [[ "$bridge_missing" -eq 0 ]]; then
  printf "  ${GREEN}✓${NC} All %d bridge modules found\n" "$bridge_ok"
else
  printf "  ${RED}✗${NC} %d/%d bridge modules found\n" "$bridge_ok" "$((bridge_ok + bridge_missing))"
  mismatches=$((mismatches + 1))
fi

echo ""
echo "Listed Invariants Verification"
echo "------------------------------"

# ─────────────────────────────────────────────────────────────────────────────
# Verify listed invariants exist in their specified files
# ─────────────────────────────────────────────────────────────────────────────

listed_invariants_ok=0
listed_invariants_missing=0

while IFS=: read -r inv file; do
  [[ -z "$inv" ]] && continue
  full_path="verification/quint/$file"
  if [[ -f "$full_path" ]] && grep -q "$inv" "$full_path" 2>/dev/null; then
    listed_invariants_ok=$((listed_invariants_ok + 1))
  else
    echo "  missing: $inv in $file"
    listed_invariants_missing=$((listed_invariants_missing + 1))
  fi
done <<< "$(get_listed_invariants)"

checks_run=$((checks_run + 1))
if [[ "$listed_invariants_missing" -eq 0 && "$listed_invariants_ok" -gt 0 ]]; then
  printf "  ${GREEN}✓${NC} All %d listed invariants found\n" "$listed_invariants_ok"
elif [[ "$listed_invariants_ok" -eq 0 ]]; then
  printf "  ${YELLOW}?${NC} No invariants listed in document tables\n"
else
  printf "  ${RED}✗${NC} %d/%d listed invariants found\n" "$listed_invariants_ok" "$((listed_invariants_ok + listed_invariants_missing))"
  mismatches=$((mismatches + 1))
fi

echo ""
echo "Listed Temporal Properties Verification"
echo "---------------------------------------"

# ─────────────────────────────────────────────────────────────────────────────
# Verify listed temporal properties exist in their specified files
# ─────────────────────────────────────────────────────────────────────────────

temporal_ok=0
temporal_missing=0

while IFS=: read -r prop file; do
  [[ -z "$prop" ]] && continue
  full_path="verification/quint/$file"
  if [[ -f "$full_path" ]] && grep -q "temporal $prop" "$full_path" 2>/dev/null; then
    temporal_ok=$((temporal_ok + 1))
  else
    echo "  missing: $prop in $file"
    temporal_missing=$((temporal_missing + 1))
  fi
done <<< "$(get_listed_temporal)"

checks_run=$((checks_run + 1))
if [[ "$temporal_missing" -eq 0 && "$temporal_ok" -gt 0 ]]; then
  printf "  ${GREEN}✓${NC} All %d listed temporal properties found\n" "$temporal_ok"
elif [[ "$temporal_ok" -eq 0 ]]; then
  printf "  ${YELLOW}?${NC} No temporal properties listed in document tables\n"
else
  printf "  ${RED}✗${NC} %d/%d listed temporal properties found\n" "$temporal_ok" "$((temporal_ok + temporal_missing))"
  mismatches=$((mismatches + 1))
fi

echo ""
echo "Listed CI Gates Verification"
echo "----------------------------"

# ─────────────────────────────────────────────────────────────────────────────
# Verify listed CI gates exist in justfile
# ─────────────────────────────────────────────────────────────────────────────

ci_gates_ok=0
ci_gates_missing=0

# Extract CI gate commands from the document tables
while IFS= read -r cmd; do
  [[ -z "$cmd" ]] && continue
  # Strip backticks and "just " prefix
  cmd="${cmd#\`}"
  cmd="${cmd%\`}"
  cmd="${cmd#just }"

  # Check if task exists in justfile (as a recipe definition)
  if grep -qE "^${cmd}:" justfile 2>/dev/null; then
    ci_gates_ok=$((ci_gates_ok + 1))
  # Special case: conformance_golden_fixtures is a cargo test, not a just task
  elif [[ "$cmd" == "conformance_golden_fixtures" ]] && grep -q "conformance_golden_fixtures" justfile 2>/dev/null; then
    ci_gates_ok=$((ci_gates_ok + 1))
  else
    echo "  missing in justfile: $cmd"
    ci_gates_missing=$((ci_gates_missing + 1))
  fi
done < <(awk '/^## CI Verification Gates/{found=1; next} found && /^## /{exit} found' "$DOC" | grep -oE '`just [^`]+`|`conformance_golden_fixtures`' | sort -u)

checks_run=$((checks_run + 1))
if [[ "$ci_gates_missing" -eq 0 && "$ci_gates_ok" -gt 0 ]]; then
  printf "  ${GREEN}✓${NC} All %d listed CI gates found in justfile\n" "$ci_gates_ok"
elif [[ "$ci_gates_ok" -eq 0 ]]; then
  printf "  ${YELLOW}?${NC} No CI gates listed in document\n"
else
  printf "  ${RED}✗${NC} %d/%d listed CI gates found\n" "$ci_gates_ok" "$((ci_gates_ok + ci_gates_missing))"
  mismatches=$((mismatches + 1))
fi

echo ""
echo "========================================"

if [[ "$mismatches" -gt 0 ]]; then
  echo -e "${RED}FAILED${NC}: $mismatches of $checks_run checks failed"
  echo ""
  echo "To fix: Update docs/998_verification_coverage.md to match actual counts,"
  echo "or investigate why the codebase counts differ from documentation."
  exit 1
else
  echo -e "${GREEN}PASSED${NC}: All $checks_run checks passed"
  exit 0
fi
