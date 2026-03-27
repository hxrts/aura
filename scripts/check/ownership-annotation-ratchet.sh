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
    mapfile -t scope_paths <<'EOF'
crates/aura-app/src/workflows
crates/aura-web/src
crates/aura-terminal/src
EOF
    required_attr='#[aura_macros::semantic_owner'
    ;;
  actor-owned)
    mapfile -t scope_paths <<'EOF'
crates/aura-agent/src/runtime/services
EOF
    required_attr='#[aura_macros::actor_owned'
    ;;
  capability-boundary)
    mapfile -t scope_paths <<'EOF'
crates/aura-app/src/workflows
crates/aura-agent/src/runtime_bridge
EOF
    required_attr='#[aura_macros::capability_boundary'
    ;;
  *)
    echo "ownership-annotation-ratchet: unknown mode: $mode" >&2
    exit 1
    ;;
esac

diff_output="$(git diff -U3 "$diff_range" -- "${scope_paths[@]}" || true)"
if [[ -z "$diff_output" ]]; then
  echo "ownership-annotation-ratchet($mode): no diff in scope"
  exit 0
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
  if (( ${#added_window[@]} > 6 )); then
    added_window=("${added_window[@]: -6}")
  fi
}

window_has_required_attr() {
  local line
  for line in "${added_window[@]}"; do
    [[ "$line" == *"$required_attr"* ]] && return 0
  done
  return 1
}

is_semantic_owner_candidate() {
  local line="$1"
  local pattern='^\+.*async[[:space:]]+fn[[:space:]]+[A-Za-z0-9_]+(_owned|_with_terminal_status)\('
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
  local pattern='^\+.*fn[[:space:]]+issue_[A-Za-z0-9_]+_proof\('
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
            echo "✖ $current_file: added runtime service appears to require $required_attr near ${line#+}" >&2
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

if (( violations > 0 )); then
  echo "ownership-annotation-ratchet($mode): $violations violation(s)" >&2
  exit 1
fi

echo "ownership-annotation-ratchet($mode): clean"
