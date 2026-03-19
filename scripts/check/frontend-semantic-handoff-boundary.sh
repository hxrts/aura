#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "frontend-semantic-handoff-boundary: $*" >&2
  exit 1
}

check_allowed_sites() {
  local pattern="$1"
  local description="$2"
  local allow_regex="$3"
  shift 3
  local hits
  hits="$(rg -n "$pattern" "$@" -g '*.rs' || true)"
  if [[ -z "$hits" ]]; then
    return
  fi

  local disallowed
  disallowed="$(printf '%s\n' "$hits" | rg -v "$allow_regex" || true)"
  if [[ -n "$disallowed" ]]; then
    printf '%s\n' "$disallowed" >&2
    fail "$description must stay within the sanctioned frontend ownership boundary"
  fi
}

submit_allow='^(crates/aura-terminal/src/tui/screens/app/shell.rs|crates/aura-terminal/src/tui/semantic_lifecycle.rs):'
handoff_allow='^(crates/aura-terminal/src/tui/callbacks/factories/chat.rs|crates/aura-terminal/src/tui/callbacks/factories/contacts.rs|crates/aura-terminal/src/tui/callbacks/factories/mod.rs|crates/aura-terminal/src/tui/semantic_lifecycle.rs):'
authoritative_state_allow='^(crates/aura-terminal/src/tui/screens/app/shell.rs|crates/aura-terminal/src/tui/state/mod.rs|crates/aura-terminal/src/tui/harness_state/mod.rs):'

check_allowed_sites \
  'SubmittedOperationOwner::submit_local_only' \
  'local semantic owner allocation' \
  "$submit_allow" \
  crates/aura-terminal \
  crates/aura-web

check_allowed_sites \
  '\.handoff_to_app_workflow\(' \
  'frontend handoff' \
  "$handoff_allow" \
  crates/aura-terminal \
  crates/aura-web

check_allowed_sites \
  'set_authoritative_operation_state\(' \
  'authoritative operation state mutation' \
  "$authoritative_state_allow" \
  crates/aura-terminal \
  crates/aura-web

echo "frontend-semantic-handoff-boundary: clean"
