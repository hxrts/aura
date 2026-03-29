#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mode="${1:-}"
if [[ -z "$mode" ]]; then
  echo "usage: scripts/check/ownership-annotation-ratchet.sh <semantic-owner|actor-owned|capability-boundary>" >&2
  exit 1
fi

diff_range="${AURA_OWNERSHIP_RATCHET_DIFF_RANGE:-}"
if [[ -z "$diff_range" ]]; then
  if [[ -n "${GITHUB_BASE_REF:-}" ]] && git rev-parse --verify "origin/${GITHUB_BASE_REF}" >/dev/null 2>&1; then
    diff_range="origin/${GITHUB_BASE_REF}...HEAD"
  elif git rev-parse --verify HEAD >/dev/null 2>&1; then
    diff_range="HEAD"
  else
    echo "ownership-annotation-ratchet($mode): unable to compute diff range; skipping"
    exit 0
  fi
fi

case "$mode" in
  semantic-owner)
    scope_paths=(
      crates/aura-app/src/workflows
      crates/aura-web/src
      crates/aura-terminal/src
    )
    required_attr='#[aura_macros::semantic_owner'
    completeness_exclusions=()
    ;;
  actor-owned)
    scope_paths=(
      crates/aura-agent/src/runtime/services
    )
    required_attr='#[aura_macros::actor_'
    completeness_exclusions=()
    ;;
  capability-boundary)
    scope_paths=(
      crates/aura-app/src/workflows
      crates/aura-agent/src/runtime_bridge
    )
    required_attr='#[aura_macros::capability_boundary'
    completeness_exclusions=()
    ;;
  *)
    echo "ownership-annotation-ratchet: unknown mode: $mode" >&2
    exit 1
    ;;
esac

exclusion_count="${#completeness_exclusions[@]}"

has_named_exclusion() {
  local key="$1"
  local entry
  for entry in "${completeness_exclusions[@]:-}"; do
    [[ "$entry" == "$key:"* ]] && return 0
  done
  return 1
}

validate_named_exclusions() {
  local entry reason
  for entry in "${completeness_exclusions[@]:-}"; do
    [[ -n "$entry" ]] || continue
    if [[ "$entry" != *:* ]]; then
      echo "ownership-annotation-ratchet($mode): invalid exclusion entry '$entry' (expected file:function:reason)" >&2
      exit 1
    fi
    reason="${entry#*:*:}"
    if [[ "$reason" == "$entry" || -z "$reason" ]]; then
      echo "ownership-annotation-ratchet($mode): exclusion '$entry' is missing a reason" >&2
      exit 1
    fi
  done
}

validate_named_exclusions

diff_output="$(git diff -U3 "$diff_range" -- "${scope_paths[@]}" || true)"
has_diff=1
if [[ -z "$diff_output" ]]; then
  has_diff=0
fi

violations=0
current_file=""
added_window=()

reset_window() {
  added_window=()
}

remember_added_line() {
  local line="$1"
  added_window+=("$line")
  if (( ${#added_window[@]} > 16 )); then
    added_window=("${added_window[@]: -16}")
  fi
}

window_has_required_attr() {
  local line
  for line in "${added_window[@]}"; do
    [[ "$line" == *"$required_attr"* ]] && return 0
  done
  return 1
}

file_has_attr_for_function() {
  local file="$1"
  local function_name="$2"
  local attr_regex="$3"
  awk -v fn="$function_name" -v attr_regex="$attr_regex" '
    { lines[NR] = $0 }
    $0 ~ "(async[[:space:]]+)?fn " fn "\\(" {
      found = 1
      for (i = NR - 1; i >= 1 && i >= NR - 16; i--) {
        if (lines[i] ~ attr_regex) {
          ok = 1
          break
        }
      }
    }
    END {
      if (!found || !ok) {
        exit 1
      }
    }
  ' "$file"
}

check_capability_boundary_completeness() {
  local attr_regex='#[[](aura_macros::)?capability_boundary'
  local -a required_entries=(
    "crates/aura-app/src/workflows/semantic_facts.rs:semantic_lifecycle_publication_capability"
    "crates/aura-app/src/workflows/semantic_facts.rs:semantic_readiness_publication_capability"
    "crates/aura-app/src/workflows/semantic_facts.rs:semantic_postcondition_proof_capability"
    "crates/aura-app/src/workflows/semantic_facts.rs:authorize_readiness_publication"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_semantic_operation_context"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_home_created_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_account_created_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_channel_membership_ready_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_invitation_created_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_invitation_exported_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_channel_invitation_created_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_invitation_accepted_or_materialized_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_pending_invitation_consumed_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_invitation_declined_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_invitation_revoked_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_device_enrollment_started_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_message_committed_proof"
    "crates/aura-app/src/workflows/semantic_facts.rs:issue_device_enrollment_imported_proof"
    "crates/aura-agent/src/runtime_bridge/mod.rs:secure_storage_bootstrap_boundary"
    "crates/aura-agent/src/runtime_bridge/mod.rs:secure_storage_bootstrap_store_capabilities"
  )
  local entry file function_name
  for entry in "${required_entries[@]}"; do
    file="${entry%%:*}"
    function_name="${entry##*:}"
    if has_named_exclusion "$file:$function_name"; then
      continue
    fi
    if ! file_has_attr_for_function "$file" "$function_name" "$attr_regex"; then
      echo "✖ $file: capability-boundary completeness requires #[aura_macros::capability_boundary] near fn $function_name(...)" >&2
      violations=$((violations + 1))
    fi
  done
}

check_actor_owned_completeness() {
  local file
  while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    if rg -n '^(pub )?struct [A-Za-z0-9_]*(Service|Manager|Coordinator|Subsystem|Actor)\b' "$file" >/dev/null; then
      if ! rg -n '^\s*#\[(aura_macros::)?actor_(owned|root)' "$file" >/dev/null; then
        echo "✖ $file: runtime service subtree completeness requires #[aura_macros::actor_owned] or #[aura_macros::actor_root]" >&2
        violations=$((violations + 1))
      fi
    fi
  done < <(find crates/aura-agent/src/runtime/services -name '*.rs' | sort)
}

check_semantic_owner_completeness() {
  local attr_regex='#[[](aura_macros::)?semantic_owner'
  local -a required_entries=(
    "crates/aura-app/src/workflows/account.rs:initialize_runtime_account_owned"
    "crates/aura-app/src/workflows/ceremonies.rs:start_device_enrollment_ceremony_owned"
    "crates/aura-app/src/workflows/context.rs:create_home_owned"
    "crates/aura-app/src/workflows/invitation.rs:accept_invitation_id_owned"
    "crates/aura-app/src/workflows/invitation.rs:accept_imported_invitation_owned"
    "crates/aura-app/src/workflows/invitation.rs:accept_pending_home_invitation_id_owned"
    "crates/aura-app/src/workflows/invitation.rs:create_channel_invitation_owned"
    "crates/aura-app/src/workflows/messaging.rs:join_channel_by_name_owned"
    "crates/aura-app/src/workflows/messaging.rs:send_message_ref_owned"
    "crates/aura-app/src/workflows/messaging.rs:invite_user_to_channel_with_context_owned"
  )
  local entry file function_name
  for entry in "${required_entries[@]}"; do
    file="${entry%%:*}"
    function_name="${entry##*:}"
    if has_named_exclusion "$file:$function_name"; then
      continue
    fi
    if ! file_has_attr_for_function "$file" "$function_name" "$attr_regex"; then
      echo "✖ $file: semantic-owner completeness requires #[aura_macros::semantic_owner] near async fn $function_name(...)" >&2
      violations=$((violations + 1))
    fi
  done
}

is_semantic_owner_candidate() {
  local line="$1"
  local pattern='^\+[[:space:]]*(pub([[:space:]]*\([^)]*\))?[[:space:]]+)?async[[:space:]]+fn[[:space:]]+[A-Za-z0-9_]+(_owned|_with_terminal_status)\('
  [[ "$current_file" == crates/aura-app/src/workflows/* \
      || "$current_file" == crates/aura-web/src/* \
      || "$current_file" == crates/aura-terminal/src/* ]] || return 1
  [[ "$line" =~ $pattern ]]
}

is_actor_owned_candidate() {
  local line="$1"
  local pattern='^\+.*struct[[:space:]]+[A-Za-z0-9_]*(Service|Manager|Coordinator|Subsystem|Actor)([[:space:]]*[{<]|$)'
  [[ "$current_file" == crates/aura-agent/src/runtime/services/* ]] || return 1
  [[ "$line" =~ $pattern ]]
}

is_capability_boundary_candidate() {
  local line="$1"
  local pattern='^\+.*fn[[:space:]]+(issue_[A-Za-z0-9_]+_(proof|context)|[A-Za-z0-9_]*capability|authorize_[A-Za-z0-9_]+|secure_storage_[A-Za-z0-9_]+)\('
  [[ "$current_file" == crates/aura-app/src/workflows/* \
      || "$current_file" == crates/aura-agent/src/runtime_bridge/* ]] || return 1
  [[ "$line" =~ $pattern ]]
}

while IFS= read -r line; do
  case "$line" in
    "+++ b/"*)
      current_file="${line#+++ b/}"
      reset_window
      ;;
    "@@"*)
      reset_window
      ;;
    "+"*)
      [[ "$line" == "+++"* ]] && continue
      remember_added_line "$line"
      case "$mode" in
        semantic-owner)
          if is_semantic_owner_candidate "$line" && ! window_has_required_attr; then
            echo "✖ $current_file: added ownership boundary appears to require $required_attr near ${line#+}" >&2
            violations=$((violations + 1))
          fi
          ;;
        actor-owned)
          if is_actor_owned_candidate "$line" && ! window_has_required_attr; then
            echo "✖ $current_file: added runtime service appears to require #[aura_macros::actor_owned] or #[aura_macros::actor_root] near ${line#+}" >&2
            violations=$((violations + 1))
          fi
          ;;
        capability-boundary)
          if is_capability_boundary_candidate "$line" && ! window_has_required_attr; then
            echo "✖ $current_file: added proof issuer appears to require $required_attr near ${line#+}" >&2
            violations=$((violations + 1))
          fi
          ;;
      esac
      ;;
    *)
      ;;
  esac
done <<< "$diff_output"

case "$mode" in
  actor-owned)
    check_actor_owned_completeness
    ;;
  semantic-owner)
    check_semantic_owner_completeness
    ;;
  capability-boundary)
    check_capability_boundary_completeness
    ;;
esac

if (( violations > 0 )); then
  echo "ownership-annotation-ratchet($mode): $violations violation(s)" >&2
  exit 1
fi

if (( has_diff == 0 )); then
  echo "ownership-annotation-ratchet($mode): no diff in scope; completeness clean (${exclusion_count} named exclusions)"
else
  echo "ownership-annotation-ratchet($mode): clean (${exclusion_count} named exclusions)"
fi
