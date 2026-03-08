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
    elif git rev-parse --verify HEAD^ >/dev/null 2>&1; then
      diff_range="HEAD^..HEAD"
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

flow_relevant=$(echo "$changed_files" | rg -n '^crates/aura-terminal/src/tui/|^crates/aura-web/src/|^crates/aura-ui/src/|^crates/aura-app/src/workflows/' || true)
if [[ -z "$flow_relevant" ]]; then
  echo "ux-flow-coverage: no flow-relevant source changes"
  exit 0
fi

declare -A FLOW_SCENARIOS
FLOW_SCENARIOS[chat]='scenarios/harness/scenario1-invitation-chat-e2e.toml scenarios/harness/scenario3-irc-slash-commands-e2e.toml scenarios/harness/scenario5-chat-modal-and-retry-e2e.toml scenarios/harness/scenario11-demo-full-tui-flow-e2e.toml scenarios/harness/scenario13-mixed-contact-channel-message-e2e.toml'
FLOW_SCENARIOS[contacts]='scenarios/harness/scenario1-invitation-chat-e2e.toml scenarios/harness/scenario2-social-topology-e2e.toml scenarios/harness/scenario6-contacts-lan-and-contact-lifecycle-e2e.toml scenarios/harness/scenario9-guardian-and-mfa-ceremonies-e2e.toml scenarios/harness/scenario13-mixed-contact-channel-message-e2e.toml'
FLOW_SCENARIOS[settings]='scenarios/harness/scenario8-settings-devices-authority-e2e.toml scenarios/harness/scenario9-guardian-and-mfa-ceremonies-e2e.toml scenarios/harness/scenario10-recovery-and-notifications-e2e.toml scenarios/harness/scenario12-mixed-device-enrollment-removal-e2e.toml'
FLOW_SCENARIOS[neighborhood]='scenarios/harness/scenario2-social-topology-e2e.toml scenarios/harness/scenario7-neighborhood-keypath-parity-e2e.toml scenarios/harness/scenario11-demo-full-tui-flow-e2e.toml'
FLOW_SCENARIOS[navigation]='scenarios/harness/scenario4-global-nav-and-help-e2e.toml scenarios/harness/scenario11-demo-full-tui-flow-e2e.toml'
FLOW_SCENARIOS[generic]='scenarios/harness/scenario1-invitation-chat-e2e.toml scenarios/harness/scenario2-social-topology-e2e.toml scenarios/harness/scenario4-global-nav-and-help-e2e.toml scenarios/harness/scenario8-settings-devices-authority-e2e.toml scenarios/harness/scenario11-demo-full-tui-flow-e2e.toml'

# Infer touched flow buckets from changed file names.
flows=()
if echo "$flow_relevant" | rg -qi 'chat|message|channel|slash|moderat|command'; then flows+=(chat); fi
if echo "$flow_relevant" | rg -qi 'contact|invite|invitation|lan'; then flows+=(contacts); fi
if echo "$flow_relevant" | rg -qi 'setting|recovery|guardian|mfa|authority|device|notification'; then flows+=(settings); fi
if echo "$flow_relevant" | rg -qi 'neighborhood|home|topology|nh'; then flows+=(neighborhood); fi
if echo "$flow_relevant" | rg -qi 'nav|help'; then flows+=(navigation); fi
if [[ ${#flows[@]} -eq 0 ]]; then flows+=(generic); fi

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

  if ! $scenarios_changed && ! $doc_touched; then
    echo "✖ flow-relevant changes detected for '$flow' without scenario or docs coverage update"
    echo "  fix: update one of:"
    for sf in $scenarios; do
      echo "    - $sf"
    done
    echo "  or update docs/997_ux_flow_coverage.md"
    violations=$((violations + 1))
  else
    echo "• coverage mapping OK for flow=$flow"
  fi
done

if [[ "$violations" -gt 0 ]]; then
  echo "ux-flow-coverage: $violations violation(s)"
  exit 1
fi

echo "ux-flow-coverage: clean"
