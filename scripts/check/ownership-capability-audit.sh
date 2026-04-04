#!/usr/bin/env bash
# Audit crate sources for forbidden ownership and capability patterns.
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

had_hits=0

run_group() {
  local title="$1"
  local pattern="$2"
  shift 2
  local output
  output="$(
    rg -n --hidden \
      --glob '!work/**' \
      --glob '!docs/book/**' \
      --glob '!crates/aura-macros/tests/boundaries/**' \
      --glob '!crates/aura-agent/tests/ui/**' \
      "$pattern" "$@" || true
  )"
  output="$(
    printf '%s\n' "$output" | rg -v '^docs/809_capability_vocabulary_inventory[.]md:' || true
  )"
  if [[ -n "$output" ]]; then
    had_hits=1
    printf '## %s\n%s\n\n' "$title" "$output"
  fi
}

run_group \
  "Product Choreography Vocabulary Drift" \
  'guard_capability = "[^"]*,[^"]+"' \
  -g '*.tell' \
  crates

run_group \
  "Docs, Examples, and .claude Legacy Capability Guidance" \
  '"send_ping"|"send_pong"|guard_capability = "send_request"|guard_capability = "respond"|permission_name|"create_session"|"join_session"|"decline_session"|"activate_session"|"broadcast_message"|"check_status"|"report_status"|"end_session"' \
  -g '*.md' \
  -g '*.rs' \
  -g '*.tell' \
  docs \
  examples \
  .claude

run_group \
  "Support and Fixture Legacy Capability Vocabulary" \
  '"invitation:create"|capability\("recovery_initiate"\)|capability\("recovery_approve"\)|capability\("threshold_sign"\)' \
  crates/aura-core/src/ownership.rs \
  crates/aura-testkit/src/fixtures/biscuit.rs

if [[ "$had_hits" -ne 0 ]]; then
  echo "capability-model-audit: remaining legacy/non-canonical hits detected" >&2
  exit 1
fi

echo "capability-model-audit: clean"
