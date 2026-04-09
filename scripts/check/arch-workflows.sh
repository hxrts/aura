#!/usr/bin/env bash
# Check aura-app workflow hygiene and docs traceability.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_workflows() {
  section "Workflow hygiene — use helpers for runtime, parsing, signals"

  local runtime_str
  runtime_str=$(rg --no-heading "Runtime bridge not available" crates/aura-app/src/workflows -g "*.rs" \
    | grep -v "crates/aura-app/src/workflows/runtime.rs" | grep -v "crates/aura-app/src/workflows/error.rs" | grep -v '\.contains(' || true)
  emit_hits "Direct runtime error strings" "$runtime_str"

  local parse_auth
  parse_auth=$(rg --no-heading "parse::<AuthorityId>" crates/aura-app/src/workflows -g "*.rs" \
    | grep -v "crates/aura-app/src/workflows/parse.rs" || true)
  emit_hits "Direct AuthorityId parsing" "$parse_auth"

  local parse_ctx
  parse_ctx=$(rg --no-heading "parse::<ContextId>" crates/aura-app/src/workflows -g "*.rs" \
    | grep -v "crates/aura-app/src/workflows/parse.rs" || true)
  emit_hits "Direct ContextId parsing" "$parse_ctx"

  local signal_access
  signal_access=$(rg --no-heading "\\.(read|emit)\\(&\\*.*_SIGNAL" crates/aura-app/src/workflows -g "*.rs" \
    | grep -v "crates/aura-app/src/workflows/signals.rs" || true)
  emit_hits "Direct signal access" "$signal_access"

  local init_calls
  init_calls=$(rg --no-heading "init_signals\\(" crates/aura-app/src -g "*.rs" \
    | grep -v "crates/aura-app/src/core/app.rs" \
    | grep -v "crates/aura-app/src/core/app/legacy.rs" \
    | grep -v "init_signals_with_hooks" || true)
  emit_hits "Direct init_signals calls" "$init_calls"

  local legacy_slash_dispatch
  legacy_slash_dispatch=$(rg --no-heading "CommandDispatcher::|CapabilityPolicy::|dispatcher\\.dispatch\\(" \
    crates/aura-terminal/src/tui/callbacks/factories -g "*.rs" || true)
  emit_hits "Forbidden slash command dispatcher usage" "$legacy_slash_dispatch"

  local legacy_slash_parse
  legacy_slash_parse=$(rg --no-heading "parse_command\\(" \
    crates/aura-terminal/src/tui/callbacks/factories -g "*.rs" || true)
  emit_hits "Forbidden slash parse helper usage" "$legacy_slash_parse"

  local legacy_input_parse
  legacy_input_parse=$(rg --no-heading "parse_command\\(" \
    crates/aura-terminal/src/tui/state/handlers/input.rs -g "*.rs" || true)
  emit_hits "Forbidden slash parse helper usage in input handler" "$legacy_input_parse"

  local legacy_dispatch_refs
  legacy_dispatch_refs=$(rg --no-heading "CommandDispatcher|CapabilityPolicy" \
    crates/aura-terminal/src -g "*.rs" \
    | grep -v "crates/aura-terminal/src/tui/effects/dispatcher.rs" \
    | grep -v "crates/aura-terminal/src/tui/effects/mod.rs" || true)
  emit_hits "Forbidden dispatcher references outside dispatcher module" "$legacy_dispatch_refs"

  local legacy_parse_refs
  legacy_parse_refs=$(rg --no-heading "parse_command\\(" \
    crates/aura-terminal/src -g "*.rs" \
    | grep -v "crates/aura-terminal/src/tui/commands.rs" || true)
  emit_hits "Forbidden parse helper references outside commands module" "$legacy_parse_refs"

  local missing_strong=false
  if ! rg -q "workflows::strong_command::execute_planned" \
    crates/aura-terminal/src/tui/callbacks/factories; then
    violation "[L7] Strong command pipeline missing: callbacks/factories must call strong_command::execute_planned"
    missing_strong=true
  fi
  if ! rg -q "strong_resolver\\.plan\\(" \
    crates/aura-terminal/src/tui/callbacks/factories; then
    violation "[L7] Strong command planning missing: callbacks/factories must plan resolved commands before execution"
    missing_strong=true
  fi
  $missing_strong || info "Strong command pipeline: enforced"

  section "Workflow legibility — typed boundaries and docs traceability"

  local string_results
  string_results=$(rg --no-heading "Result<[^>]*,\s*String>" crates/aura-app/src/workflows -g "*.rs" \
    | grep -Ev "crates/aura-app/src/workflows/(authority|budget|chat_commands)\\.rs:" || true)
  emit_hits "Untyped workflow result (Result<_, String>)" "$string_results"

  local json_value_hits
  json_value_hits=$(rg --no-heading "serde_json::Value" crates/aura-app/src/workflows -g "*.rs" \
    | grep -Ev "crates/aura-app/src/workflows/recovery_cli\\.rs:" || true)
  emit_hits "Stringly JSON workflow surface (serde_json::Value)" "$json_value_hits"

  local diff_range=""
  if git rev-parse --verify HEAD^2 >/dev/null 2>&1; then
    diff_range="HEAD^1...HEAD"
  elif git rev-parse --verify HEAD^ >/dev/null 2>&1; then
    diff_range="HEAD^..HEAD"
  fi

  if [[ -n "$diff_range" ]]; then
    local added_surfaces docs_touch
    added_surfaces=$(git diff --name-status --diff-filter=A "$diff_range" \
      | awk '{print $2}' \
      | grep -E "^crates/aura-app/src/workflows/.*\\.rs$|^crates/aura-terminal/src/tui/effects/.*\\.rs$" || true)
    if [[ -n "$added_surfaces" ]]; then
      docs_touch=$(git diff --name-only "$diff_range" \
        | grep -E "^docs/(116_cli_tui\\.md|804_testing_guide\\.md|807_system_internals_guide\\.md|997_ux_flow_coverage\\.md|999_project_structure\\.md)$" || true)
      if [[ -z "$docs_touch" ]]; then
        emit_hits "New workflow surface without docs update" "$added_surfaces"
        hint "When adding workflow/tui effect surface files, update at least one architecture/testing doc."
      else
        info "Workflow docs traceability: docs updated alongside new surfaces"
      fi
    else
      info "Workflow docs traceability: no new workflow/tui effect surfaces"
    fi
  else
    info "Workflow docs traceability: skipped (insufficient git history)"
  fi
}

check_workflows
