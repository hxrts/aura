#!/usr/bin/env bash
# Check TUI reactive data model and fact-commit synchronization.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_reactive() {
  section "Reactive model — signals are source of truth; no domain data in props"

  local domain_in_props
  domain_in_props=$(rg --no-heading -l "// === Domain data" crates/aura-terminal/src/tui/screens -g "*.rs" || true)
  if [[ -n "$domain_in_props" ]]; then
    local count
    count=$(echo "$domain_in_props" | wc -l | tr -d ' ')
    violation "[L7] Domain data in props: $count screen(s)"
    hint "Remove domain fields from Props; subscribe to signals"
  else
    info "Domain data in props: none"
  fi

  local screens_dir="crates/aura-terminal/src/tui/screens"
  if [[ -d "$screens_dir" ]]; then
    local missing=""
    for f in $(find "$screens_dir" -name "screen.rs" 2>/dev/null); do
      grep -q "subscribe_signal_with_retry\|SIGNAL" "$f" 2>/dev/null || missing+="$f"$'\n'
    done
    emit_hits "Screen without signal subscription" "$missing"
  fi

  verbose "Props: only view state (focus, selection), callbacks, config"

  section "Fact commit sync — await view updates after commit"

  local commit_allow="crates/aura-testkit/|/tests/|_test\\.rs|crates/aura-simulator/|crates/aura-sync/|handlers/shared\\.rs"
  local commit_files missing_sync=""

  commit_files=$(rg -l "commit_generic_fact_bytes" crates -g "*.rs" | grep -Ev "$commit_allow" || true)

  for f in $commit_files; do
    [[ -z "$f" ]] && continue
    if ! grep -qE "await_next_view_update|fire_and_forget|FactCommitResult" "$f" 2>/dev/null; then
      if grep -qE "impl.*Handler|impl.*Service|async fn (accept|create|import|send)" "$f" 2>/dev/null; then
        missing_sync+="$f"$'\n'
      fi
    fi
  done

  if [[ -n "$missing_sync" ]]; then
    emit_hits "Fact commit without view sync" "$missing_sync"
    hint "Add await_next_view_update() after commit, or use FactCommitResult pattern"
  else
    info "Fact commit sync: all commits synchronized"
  fi
}

check_reactive
