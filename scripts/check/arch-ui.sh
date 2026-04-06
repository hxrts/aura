#!/usr/bin/env bash
# Check UI boundary: terminal uses aura_app::ui facade only.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_ui() {
  section "UI boundary — aura-terminal uses aura_app::ui facade only"

  local app_access
  app_access=$(rg --no-heading "aura_app::(workflows|signal_defs|views|runtime_bridge|authorization)" crates/aura-terminal/src -g "*.rs" | grep -v "///" | grep -v "//" || true)
  emit_hits "Direct aura_app module access" "$app_access"

  local view_access
  view_access=$(rg --no-heading "\\.views\\(" crates/aura-terminal/src -g "*.rs" || true)
  emit_hits "Direct ViewState access" "$view_access"

  local journal_hits
  journal_hits=$(rg --no-heading "FactRegistry|FactReducer|RelationalFact|JournalEffects|commit_.*facts|RuntimeBridge::commit" crates/aura-terminal/src -g "*.rs" | grep -v "crates/aura-terminal/src/demo/" || true)
  emit_hits "Direct journal/protocol mutation" "$journal_hits"

  local forbidden
  forbidden=$(rg --no-heading "aura_(journal|protocol|consensus|guards|amp|anti_entropy|transport|recovery|sync|invitation|authentication|relational|chat)::" crates/aura-terminal/src -g "*.rs" \
    | grep -v "/demo/" \
    | grep -v "/scenarios/" || true)
  emit_hits "Direct protocol/domain crate usage" "$forbidden"

  section "Terminal time — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for direct wall-clock usage in aura-terminal."

  section "Terminal business logic — keep in aura_app::workflows"

  local domain_state
  domain_state=$(rg --no-heading "HashSet<.*Id>|HashMap<.*Id," crates/aura-terminal/src/handlers -g "*.rs" \
    | grep -v "// temporary" | grep -v "// local cache" | grep -Ev "/tests/" || true)
  emit_hits "Local domain state in handlers" "$domain_state"
}

check_ui
