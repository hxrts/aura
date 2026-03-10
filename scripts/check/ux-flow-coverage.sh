#!/usr/bin/env bash
set -euo pipefail

if [[ "${AURA_ALLOW_FLOW_COVERAGE_SKIP:-0}" == "1" ]]; then
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

flow_relevant=$(echo "$changed_files" | rg -n '^crates/aura-terminal/src/tui/|^crates/aura-web/src/|^crates/aura-ui/src/|^crates/aura-app/src/workflows/' || true)
if [[ -z "$flow_relevant" ]]; then
  echo "ux-flow-coverage: no flow-relevant source changes"
  exit 0
fi

declare -A FLOW_SCENARIOS
FLOW_SCENARIOS[chat]='scenarios/harness/scenario13-mixed-contact-channel-message-e2e.toml'
FLOW_SCENARIOS[contacts]='scenarios/harness/scenario13-mixed-contact-channel-message-e2e.toml'
FLOW_SCENARIOS[settings]='scenarios/harness/shared-settings-parity.toml scenarios/harness/scenario12-mixed-device-enrollment-removal-e2e.toml'
FLOW_SCENARIOS[neighborhood]='scenarios/harness/real-runtime-mixed-startup-smoke.toml'
FLOW_SCENARIOS[notifications]='scenarios/harness/scenario10-recovery-and-notifications-e2e.toml'

flows=()
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  case "$file" in
    crates/aura-app/src/workflows/messaging.rs|\
    crates/aura-terminal/src/tui/screens/chat/*|\
    crates/aura-terminal/src/tui/state/views/chat.rs)
      flows+=(chat)
      ;;
    crates/aura-app/src/workflows/invitation.rs|\
    crates/aura-terminal/src/tui/screens/contacts/*|\
    crates/aura-terminal/src/tui/state/views/contacts.rs)
      flows+=(contacts)
      ;;
    crates/aura-app/src/workflows/settings.rs|\
    crates/aura-app/src/workflows/recovery*|\
    crates/aura-terminal/src/tui/screens/settings/*|\
    crates/aura-terminal/src/tui/state/views/settings.rs|\
    crates/aura-terminal/src/tui/screens/recovery/*|\
    crates/aura-terminal/src/tui/state/views/recovery.rs)
      flows+=(settings notifications)
      ;;
    crates/aura-terminal/src/tui/screens/neighborhood/*|\
    crates/aura-terminal/src/tui/state/views/neighborhood.rs|\
    crates/aura-app/src/workflows/context.rs)
      flows+=(neighborhood)
      ;;
    crates/aura-terminal/src/tui/screens/notifications/*|\
    crates/aura-terminal/src/tui/state/views/notifications.rs)
      flows+=(notifications)
      ;;
    crates/aura-ui/src/*|crates/aura-web/src/*)
      flows+=(chat contacts settings neighborhood notifications)
      ;;
  esac
done <<< "$changed_files"

if [[ ${#flows[@]} -eq 0 ]]; then
  echo "ux-flow-coverage: no mapped flow domains for changed files"
  exit 0
fi

# Unique flow buckets.
mapfile -t flows < <(printf '%s
' "${flows[@]}" | sort -u)

violations=0

for flow in "${flows[@]}"; do
  scenarios="${FLOW_SCENARIOS[$flow]}"
  if [[ -z "$scenarios" ]]; then
    echo "✖ internal error: no scenario mapping for flow=$flow"
    violations=$((violations + 1))
    continue
  fi

  # Ensure mapping exists in docs.
  for sf in $scenarios; do
    if ! rg -q "`$sf`|$sf" "$COVERAGE_DOC"; then
      echo "✖ docs mapping missing for flow=$flow scenario=$sf"
      violations=$((violations + 1))
    fi
  done

  scenarios_changed=false
  for sf in $scenarios; do
    if echo "$changed_files" | rg -q "^${sf}$"; then
      scenarios_changed=true
      break
    fi
  done

  if ! $scenarios_changed && ! $coverage_metadata_touched; then
    echo "✖ flow-relevant changes detected for '$flow' without scenario or shared coverage metadata update"
    echo "  fix: update one of:"
    for sf in $scenarios; do
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
