#!/usr/bin/env bash
# Verify ceremony completions commit corresponding facts.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_ceremonies() {
  section "Ceremony facts — operations that affect UI must commit facts"

  local ceremony_allow="crates/aura-testkit/|/tests/|_test\\.rs|crates/aura-simulator/"
  local ceremony_exclude="/views/|/core/app\\.rs|ceremony_tracker\\.rs|ceremony_processor"
  local ceremony_files missing_facts=""

  ceremony_files=$(rg -l "ceremony.*complete|GuardianBinding|invitation.*accept" \
    crates/aura-agent/src/runtime_bridge -g "*.rs" \
    | grep -Ev "$ceremony_allow" || true)

  for f in $ceremony_files; do
    [[ -z "$f" ]] && continue
    if grep -qE "ceremony.*complet|guardian.*accept|Committed.*Binding" "$f" 2>/dev/null; then
      if ! grep -qE "commit_relational_facts|RelationalFact::" "$f" 2>/dev/null; then
        missing_facts+="$f -- ceremony completion without fact commit"$'\n'
      fi
    fi
  done

  local handler_files
  handler_files=$(rg -l "async fn.*ceremony|execute.*ceremony" \
    crates/aura-agent/src/handlers -g "*.rs" \
    | grep -Ev "$ceremony_allow" || true)

  for f in $handler_files; do
    [[ -z "$f" ]] && continue
    if ! grep -qE "commit_relational_facts|runtime_bridge|RelationalFact::" "$f" 2>/dev/null; then
      if grep -qE "ceremony.*complet|Ok\\(CeremonyResult" "$f" 2>/dev/null; then
        missing_facts+="$f -- ceremony handler without fact commit or delegation"$'\n'
      fi
    fi
  done

  if [[ -n "$missing_facts" ]]; then
    emit_hits "Ceremony without fact commit" "$missing_facts"
    hint "Commit RelationalFact after ceremony completion to update signal views"
  else
    info "Ceremony facts: all ceremony completions commit facts"
  fi
}

check_ceremonies
