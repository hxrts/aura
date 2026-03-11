#!/usr/bin/env bash
set -euo pipefail

if [[ "${AURA_ALLOW_FLOW_COVERAGE_SKIP:-0}" == "1" ]]; then
  if [[ "${CI:-}" == "true" || "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "ux-flow-coverage: AURA_ALLOW_FLOW_COVERAGE_SKIP=1 is not allowed in CI"
    exit 1
  fi
  echo "ux-flow-coverage: skipped via AURA_ALLOW_FLOW_COVERAGE_SKIP=1"
  exit 0
fi

COVERAGE_DOC="docs/997_ux_flow_coverage.md"
if [[ ! -f "$COVERAGE_DOC" ]]; then
  echo "missing coverage doc: $COVERAGE_DOC"
  exit 1
fi

if [[ -n "${AURA_FLOW_COVERAGE_CHANGED_FILES:-}" ]]; then
  changed_files="$AURA_FLOW_COVERAGE_CHANGED_FILES"
else
  diff_range="${AURA_FLOW_COVERAGE_DIFF_RANGE:-}"
  if [[ -z "$diff_range" ]]; then
    if [[ -n "${GITHUB_BASE_REF:-}" ]] && git rev-parse --verify "origin/${GITHUB_BASE_REF}" >/dev/null 2>&1; then
      diff_range="origin/${GITHUB_BASE_REF}...HEAD"
    elif git rev-parse --verify HEAD >/dev/null 2>&1; then
      diff_range="HEAD"
    else
      echo "ux-flow-coverage: unable to compute diff range; skipping"
      exit 0
    fi
  fi
  changed_files=$(git diff --name-only "$diff_range" || true)
fi

if [[ -z "${changed_files//[[:space:]]/}" ]]; then
  echo "ux-flow-coverage: no changed files"
  exit 0
fi

doc_touched=false
ux_doc_path='docs/997_ux_flow_coverage.md'
if echo "$changed_files" | rg -F -q "$ux_doc_path"; then
  doc_touched=true
fi
coverage_metadata_touched=false
if echo "$changed_files" | rg -q '^crates/aura-app/src/ui_contract.rs$'; then
  coverage_metadata_touched=true
fi
UI_CONTRACT="crates/aura-app/src/ui_contract.rs"
if [[ ! -f "$UI_CONTRACT" ]]; then
  echo "ux-flow-coverage: missing typed flow metadata source: $UI_CONTRACT"
  exit 1
fi

mapfile -t FLOW_SOURCE_AREAS < <(
  awk '
    /pub const SHARED_FLOW_SOURCE_AREAS/ { in_block=1; next }
    in_block && /flow: SharedFlowId::/ {
      flow=$0
      sub(/.*SharedFlowId::/, "", flow)
      sub(/,.*/, "", flow)
    }
    in_block && /path: "/ {
      path=$0
      sub(/.*path: "/, "", path)
      sub(/".*/, "", path)
      print flow "|" path
    }
    in_block && /^\];/ { in_block=0 }
  ' "$UI_CONTRACT"
)

mapfile -t FLOW_SCENARIO_COVERAGE < <(
  awk '
    /pub const SHARED_FLOW_SCENARIO_COVERAGE/ { in_block=1; next }
    in_block && /flow: SharedFlowId::/ {
      flow=$0
      sub(/.*SharedFlowId::/, "", flow)
      sub(/,.*/, "", flow)
    }
    in_block && /scenario_id: "/ {
      scenario=$0
      sub(/.*scenario_id: "/, "", scenario)
      sub(/".*/, "", scenario)
      print flow "|" scenario
    }
    in_block && /^\];/ { in_block=0 }
  ' "$UI_CONTRACT"
)

if [[ ${#FLOW_SOURCE_AREAS[@]} -eq 0 ]]; then
  echo "ux-flow-coverage: no typed shared flow source metadata found"
  exit 1
fi

if [[ ${#FLOW_SCENARIO_COVERAGE[@]} -eq 0 ]]; then
  echo "ux-flow-coverage: no typed shared flow scenario coverage metadata found"
  exit 1
fi

flows=()
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  while IFS='|' read -r flow path; do
    [[ -n "$flow" && -n "$path" ]] || continue
    if [[ "$file" == "$path" ]]; then
      flows+=("$flow")
    fi
  done < <(printf '%s\n' "${FLOW_SOURCE_AREAS[@]}")
done <<< "$changed_files"

if [[ ${#flows[@]} -eq 0 ]]; then
  echo "ux-flow-coverage: no typed shared-flow source mappings for changed files"
  exit 0
fi

# Unique flow buckets.
mapfile -t flows < <(printf '%s
' "${flows[@]}" | sort -u)

violations=0

for flow in "${flows[@]}"; do
  mapfile -t scenario_ids < <(
    printf '%s\n' "${FLOW_SCENARIO_COVERAGE[@]}" \
      | awk -F'|' -v flow="$flow" '$1 == flow { print $2 }'
  )
  scenarios=()
  for scenario_id in "${scenario_ids[@]}"; do
    scenarios+=("scenarios/harness/${scenario_id}.toml")
  done
  if [[ ${#scenarios[@]} -eq 0 ]]; then
    echo "✖ internal error: no scenario mapping for flow=$flow"
    violations=$((violations + 1))
    continue
  fi

  # Ensure mapping exists in docs.
  for sf in "${scenarios[@]}"; do
    if ! rg -q "`$sf`|$sf" "$COVERAGE_DOC"; then
      echo "✖ docs mapping missing for flow=$flow scenario=$sf"
      violations=$((violations + 1))
    fi
  done

  scenarios_changed=false
  for sf in "${scenarios[@]}"; do
    if echo "$changed_files" | rg -q "^${sf}$"; then
      scenarios_changed=true
      break
    fi
  done

  if ! $scenarios_changed && ! $coverage_metadata_touched; then
    echo "✖ flow-relevant changes detected for '$flow' without scenario or shared coverage metadata update"
    echo "  fix: update one of:"
    for sf in "${scenarios[@]}"; do
      echo "    - $sf"
    done
    echo "  or update crates/aura-app/src/ui_contract.rs coverage metadata"
    violations=$((violations + 1))
  else
    echo "• coverage mapping OK for flow=$flow"
  fi
done

if $doc_touched && ! $coverage_metadata_touched; then
  echo "• coverage doc updated for traceability"
fi

if [[ "$violations" -gt 0 ]]; then
  echo "ux-flow-coverage: $violations violation(s)"
  exit 1
fi

echo "ux-flow-coverage: clean"
