#!/usr/bin/env bash
# Check effect system governance, crypto boundaries, and concurrency hygiene.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_effects() {
  section "Effects syntax and escape hatches — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for effect placement, runtime coupling, and impure/time/random checks."

  section "VM bridge discipline — bridge state and ownership stay centralized"

  local ad_hoc_vm_bridge_hits unreviewed_envelope_mode_hits
  ad_hoc_vm_bridge_hits=$(rg --no-heading -n "Mutex<.*VmBridgePendingSend|Mutex<.*VmBridgeBlockedEdge|Mutex<.*VmBridgeSchedulerSignals|VecDeque<.*VmBridgePendingSend|VecDeque<.*VmBridgeBlockedEdge|VecDeque<.*VmBridgeSchedulerSignals" \
    crates/aura-agent/src crates/aura-testkit/src -g "*.rs" \
    | grep -v "crates/aura-agent/src/runtime/subsystems/vm_bridge.rs" \
    | grep -v "crates/aura-testkit/src/stateful_effects/vm_bridge.rs" || true)
  ad_hoc_vm_bridge_hits=$(filter_test_modules "$ad_hoc_vm_bridge_hits")
  emit_hits "Ad hoc VM bridge queue/state storage outside VmBridgeEffects implementations" "$ad_hoc_vm_bridge_hits"

  unreviewed_envelope_mode_hits=$(rg --no-heading -n "ThreadedVM::with_workers|AuraVmRuntimeMode::ThreadedReplayDeterministic|AuraVmRuntimeMode::ThreadedEnvelopeBounded" \
    crates/aura-agent/src -g "*.rs" \
    | grep -v "crates/aura-agent/src/runtime/choreo_engine.rs" \
    | grep -v "crates/aura-agent/src/runtime/vm_hardening.rs" || true)
  unreviewed_envelope_mode_hits=$(filter_test_modules "$unreviewed_envelope_mode_hits")
  emit_hits "Unreviewed threaded or envelope runtime selection outside hardening/engine paths" "$unreviewed_envelope_mode_hits"

  section "OTA scope model — no network-wide authoritative cutover in code"

  local ota_global_cutover_hits
  ota_global_cutover_hits=$(rg --no-heading -n "GlobalNetwork|NetworkWide|network-wide authoritative cutover|global cutover|whole Aura network.*cutover" \
    crates/aura-maintenance/src \
    crates/aura-sync/src/services \
    crates/aura-agent/src/runtime/services/ota_manager.rs \
    -g "*.rs" || true)
  ota_global_cutover_hits=$(filter_test_modules "$ota_global_cutover_hits")
  emit_hits "OTA code assumes a network-wide authoritative cutover model" "$ota_global_cutover_hits"

  section "Impure/time/random syntax — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for std::fs/std::net/runtime/time/random/sleep checks."

  section "Conformance envelope registry — classify every effect envelope kind"

  local registry_file classified_kinds duplicate_classified
  registry_file="crates/aura-core/src/conformance.rs"
  if [[ ! -f "$registry_file" ]]; then
    violation "Missing conformance registry file: $registry_file"
    hint "Add AURA_EFFECT_ENVELOPE_CLASSIFICATIONS in aura-core and classify each effect kind."
  else
    classified_kinds=$(rg --no-heading '^\s*\(".*",\s*AuraEnvelopeLawClass::' "$registry_file" \
      | sed -E 's/^[[:space:]]*\("([^"]+)".*/\1/' | sort -u || true)

    if [[ -z "$classified_kinds" ]]; then
      violation "No classified effect envelope kinds found in $registry_file"
      hint "Populate AURA_EFFECT_ENVELOPE_CLASSIFICATIONS with strict/commutative/algebraic entries."
    else
      duplicate_classified=$(rg --no-heading '^\s*\(".*",\s*AuraEnvelopeLawClass::' "$registry_file" \
        | sed -E 's/^[[:space:]]*\("([^"]+)".*/\1/' | sort | uniq -d || true)
      if [[ -n "$duplicate_classified" ]]; then
        violation "Duplicate effect envelope classifications found: $(echo "$duplicate_classified" | tr '\n' ' ' | sed 's/  */ /g')"
      fi

      local telltale_src upstream_kinds missing
      telltale_src=$(find "$HOME/.cargo/registry/src" -type d -path "*telltale-machine-*/src" 2>/dev/null | sort -V | tail -n1 || true)

      if [[ -n "$telltale_src" && -d "$telltale_src" ]]; then
        upstream_kinds=$(rg --no-heading 'effect_kind:\s*"[^"]+"' \
          "$telltale_src/commit_common.rs" \
          "$telltale_src/effect/recording_impl.rs" \
          "$telltale_src/threaded/topology_and_planner.rs" \
          "$telltale_src/engine/topology_and_dispatch.rs" \
          | sed -E 's/.*effect_kind:[[:space:]]*"([^"]+)".*/\1/' | sort -u || true)

        if [[ -n "$upstream_kinds" ]]; then
          missing=$(comm -23 <(echo "$upstream_kinds") <(echo "$classified_kinds") || true)
          if [[ -n "$missing" ]]; then
            violation "Unclassified telltale-machine effect kinds: $(echo "$missing" | tr '\n' ' ' | sed 's/  */ /g')"
            hint "Add missing kinds to AURA_EFFECT_ENVELOPE_CLASSIFICATIONS in crates/aura-core/src/conformance.rs."
          else
            info "Effect envelope registry: all telltale-machine effect kinds are classified"
          fi
        else
          info "Effect envelope registry: telltale-machine effect_kind scan returned no kinds"
        fi
      else
        info "Effect envelope registry: telltale-machine source not found in cargo registry; skipping upstream parity check"
      fi
    fi
  fi

  section "aura-sync runtime neutrality"
  info "Run 'just lint-arch-syntax' for aura-sync runtime-neutrality enforcement."

  section "Simulation surfaces — inject via effects"
  info "Run 'just lint-arch-syntax' for simulation-surface syntax guardrails."

  section "Pure interpreter — migrate to GuardSnapshot + EffectCommand"

  local guard_sync
  guard_sync=$(rg --no-heading "GuardEffectSystem|futures::executor::block_on" crates -g "*.rs" \
    | grep -v "crates/aura-app/src/frontend_primitives/submitted_operation.rs:" \
    | grep -v "crates/aura-terminal/src/tui/semantic_lifecycle.rs:" \
    | grep -v "crates/aura-ui/src/semantic_lifecycle.rs:" \
    | sort -u || true)
  emit_hits "Synchronous guard/effect bridges" "$guard_sync"

  section "Identifier determinism — avoid entropy-consuming IDs"
  info "Run 'just lint-arch-syntax' for entropy-consuming ID and direct randomness checks."
}

check_crypto() {
  section "Crypto boundaries — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for direct crypto/randomness boundary enforcement."
}

check_concurrency() {
  section "Concurrency — delegated to Rust-native lint path"
  info "Run 'just lint-arch-syntax' for block_in_place / block_on / unbounded-channel checks."
}

check_effects
check_crypto
check_concurrency
