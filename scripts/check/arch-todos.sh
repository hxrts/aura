#!/usr/bin/env bash
# Detect TODOs, placeholders, and incomplete markers in crate source.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_todos() {
  section "Placeholders — replace nil UUIDs with real derivations"
  local placeholder_hits
  placeholder_hits=$(rg --no-heading -i "uuid::nil\\(\\)|placeholder implementation" crates -g "*.rs" \
    | grep -Ev "/tests/|/benches/|/examples/|crates/aura-simulator/|/scenarios/|/demo/" || true)
  if [[ -n "$placeholder_hits" ]]; then
    local formatted
    formatted=$(echo "$placeholder_hits" | while read -r e; do [[ -n "$e" ]] && echo "$e -- derive real IDs"; done)
    emit_hits "Placeholder ID" "$formatted"
  else
    info "Placeholder IDs: none"
  fi

  section "Deterministic algorithm TODOs"
  local det_hits
  det_hits=$(rg --no-heading -i "deterministic algorithm" crates -g "*.rs" | grep -Ev "/tests/|/benches/|/examples/" || true)
  if [[ -n "$det_hits" ]]; then
    local formatted
    formatted=$(echo "$det_hits" | while read -r e; do [[ -n "$e" ]] && echo "$e -- implement per spec"; done)
    emit_hits "Deterministic algorithm stub" "$formatted"
  else
    info "Deterministic stubs: none"
  fi

  section "Temporary context fallbacks"
  local temp_hits
  temp_hits=$(rg --no-heading -i "temporary context|temp context" crates -g "*.rs" | grep -Ev "/tests/|/benches/|/examples/" || true)
  if [[ -n "$temp_hits" ]]; then
    local formatted
    formatted=$(echo "$temp_hits" | while read -r e; do [[ -n "$e" ]] && echo "$e -- resolve via journal state"; done)
    emit_hits "Temporary context" "$formatted"
  else
    info "Temporary contexts: none"
  fi

  section "TODO/FIXME markers"
  local platform_allow="crates/aura-agent/src/builder/android.rs|crates/aura-agent/src/builder/ios.rs|crates/aura-agent/src/builder/web.rs"
  local tui_allow="Implement channel deletion callback|Implement contact removal callback|Implement invitation revocation callback|Pass actual channel"
  local chaos_allow="tree_chaos.rs.*Re-enable when chaos testing infrastructure is ready"
  local todo_hits
  todo_hits=$(rg --no-heading "TODO|FIXME" crates -g "*.rs" \
    | grep -Ev "/benches/" \
    | grep -Ev "$platform_allow" \
    | grep -Ev "$tui_allow" \
    | grep -Ev "$chaos_allow" || true)
  emit_hits "TODO/FIXME" "$todo_hits"

  section "Incomplete/WIP markers"
  local incomplete_pattern="in production[^\\n]*(would|should|not)|in a full implementation|stub|not implemented|unimplemented|temporary|workaround|hacky|\\bWIP\\b|\\bTBD\\b|prototype|future work|to be implemented"
  local stub_allow="biscuit_capability_stub|in production this would be the actual|effects/dispatcher.rs.*[Ss]tub|effects/dispatcher.rs.*[Ii]n production"
  local incomplete_hits
  incomplete_hits=$(rg --no-heading -i "$incomplete_pattern" crates -g "*.rs" \
    | grep -Ev "/tests/|/benches/|/examples/|/bin/" \
    | grep -Ev "$stub_allow" \
    | grep -E "//" || true)
  [[ -n "$incomplete_hits" ]] && emit_hits "Incomplete/WIP" "$incomplete_hits" || info "Incomplete markers: none"
}

check_todos
