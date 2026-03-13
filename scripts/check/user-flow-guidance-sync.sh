#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

if [[ -n "${AURA_UX_GUIDANCE_CHANGED_FILES:-}" ]]; then
  changed_files="$AURA_UX_GUIDANCE_CHANGED_FILES"
else
  diff_range="${AURA_UX_GUIDANCE_DIFF_RANGE:-}"
  if [[ -z "$diff_range" ]]; then
    if [[ -n "${GITHUB_BASE_REF:-}" ]] && git rev-parse --verify "origin/${GITHUB_BASE_REF}" >/dev/null 2>&1; then
      diff_range="origin/${GITHUB_BASE_REF}...HEAD"
    elif git rev-parse --verify HEAD >/dev/null 2>&1; then
      diff_range="HEAD"
    else
      echo "user-flow-guidance-sync: unable to compute diff range; skipping"
      exit 0
    fi
  fi
  changed_files="$(git diff --name-only "$diff_range" || true)"
fi

if [[ -z "${changed_files//[[:space:]]/}" ]]; then
  echo "user-flow-guidance-sync: no changed files"
  exit 0
fi


changed_list=()
while IFS= read -r file; do
  [[ -n "$file" ]] || continue
  changed_list+=("$file")
done <<< "$changed_files"

canonical_changed_path() {
  case "$1" in
    docs/997_ux_flow_coverage.md) echo "docs/997_flow_coverage.md" ;;
    scripts/check/ux-flow-coverage.sh) echo "scripts/check/user-flow-coverage.sh" ;;
    scripts/check/ux-guidance-sync.sh) echo "scripts/check/user-flow-guidance-sync.sh" ;;
    scripts/check/ux-policy-guardrails.sh) echo "scripts/check/user-flow-policy-guardrails.sh" ;;
    *) echo "$1" ;;
  esac
}

has_changed() {
  local target="$1"
  local file
  for file in "${changed_list[@]}"; do
    [[ "$(canonical_changed_path "$file")" == "$target" ]] && return 0
  done
  return 1
}

matches_any_rule_source() {
  local rule_id="$1"
  local file
  for file in "${changed_list[@]}"; do
    case "$rule_id:$(canonical_changed_path "$file")" in
      testing_guide_sync:crates/aura-app/src/ui_contract.rs|\
      testing_guide_sync:crates/aura-harness/src/*|\
      testing_guide_sync:crates/aura-harness/playwright-driver/*|\
      testing_guide_sync:crates/aura-terminal/src/tui/*|\
      testing_guide_sync:crates/aura-ui/src/*|\
      testing_guide_sync:crates/aura-web/src/*|\
      testing_guide_sync:scripts/check/shared-flow-policy.sh|\
      testing_guide_sync:scripts/check/ui-parity-contract.sh|\
      testing_guide_sync:scripts/check/harness-ui-state-evented.sh|\
      testing_guide_sync:scripts/check/user-flow-policy-guardrails.sh)
        return 0
        ;;
      coverage_report_sync:crates/aura-app/src/ui_contract.rs|\
      coverage_report_sync:scenarios/harness/*|\
      coverage_report_sync:scenarios/harness_inventory.toml|\
      coverage_report_sync:scripts/check/user-flow-coverage.sh)
        return 0
        ;;
      agent_guidance_sync:scripts/check/shared-flow-policy.sh|\
      agent_guidance_sync:scripts/check/ui-parity-contract.sh|\
      agent_guidance_sync:scripts/check/harness-ui-state-evented.sh|\
      agent_guidance_sync:scripts/check/user-flow-guidance-sync.sh|\
      agent_guidance_sync:scripts/check/user-flow-policy-guardrails.sh)
        return 0
        ;;
      skills_guidance_sync:scripts/check/shared-flow-policy.sh|\
      skills_guidance_sync:scripts/check/ui-parity-contract.sh|\
      skills_guidance_sync:scripts/check/harness-ui-state-evented.sh|\
      skills_guidance_sync:scripts/check/user-flow-guidance-sync.sh|\
      skills_guidance_sync:scripts/check/user-flow-policy-guardrails.sh)
        return 0
        ;;
    esac
  done
  return 1
}

check_rule() {
  local rule_id="$1"
  local description="$2"
  shift 2
  local missing=()
  local target

  matches_any_rule_source "$rule_id" || return 1

  for target in "$@"; do
    if [[ "$target" == .claude/* && ! -e ".claude" ]]; then
      continue
    fi
    if [[ "$target" == .claude/* && ! -e "$target" ]]; then
      continue
    fi
    if [[ "$target" == .claude/* ]] && ! git ls-files --error-unmatch "$target" >/dev/null 2>&1; then
      continue
    fi
    has_changed "$target" || missing+=("$target")
  done

  if [[ "${#missing[@]}" -gt 0 ]]; then
    echo "✖ $rule_id: missing required updates"
    echo "  $description"
    printf '  - %s\n' "${missing[@]}"
    return 2
  fi

  echo "• $rule_id: required guidance updates present"
  return 0
}

triggered=0
violations=0

if matches_any_rule_source testing_guide_sync; then
  triggered=$((triggered + 1))
  check_rule \
    testing_guide_sync \
    "Shared UX contract, determinism, and parity-surface changes must update the testing guide." \
    docs/804_testing_guide.md || violations=$((violations + 1))
fi

if matches_any_rule_source coverage_report_sync; then
  triggered=$((triggered + 1))
  check_rule \
    coverage_report_sync \
    "Shared-flow coverage, scenario inventory, parity classification changes, and release/update matrix changes must update the user flow coverage report." \
    docs/997_flow_coverage.md || violations=$((violations + 1))
fi

if matches_any_rule_source agent_guidance_sync; then
  triggered=$((triggered + 1))
  check_rule \
    agent_guidance_sync \
    "Changes to shared UX contributor policy must update AGENTS guidance." \
    AGENTS.md || violations=$((violations + 1))
fi

if matches_any_rule_source skills_guidance_sync; then
  triggered=$((triggered + 1))
  check_rule \
    skills_guidance_sync \
    "Changes to shared UX contributor policy must update local skills when the .claude workspace exists." \
    .claude/skills/testing/SKILL.md \
    .claude/skills/harness-run/SKILL.md \
    .claude/skills/aura-quick-ref/SKILL.md || violations=$((violations + 1))
fi

if [[ "$triggered" -eq 0 ]]; then
  echo "user-flow-guidance-sync: no mapped shared-user-flow guidance changes"
  exit 0
fi

if [[ "$violations" -gt 0 ]]; then
  echo "user-flow-guidance-sync: $violations violation(s)"
  exit 1
fi

echo "user-flow-guidance-sync: clean"
