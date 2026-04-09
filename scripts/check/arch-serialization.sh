#!/usr/bin/env bash
# Check wire-format serialization and runtime handler hygiene.
[[ -z "${_ARCH_LIB_LOADED:-}" ]] && source "$(dirname "$0")/arch-lib.sh"

check_serialization() {
  section "Serialization — use DAG-CBOR; no bincode"
  info "Run 'just lint-arch-syntax' for bincode usage and syntax-owned serialization/style checks."

  local wire_files non_canonical=""
  wire_files=$(find crates -type f \( -name "wire.rs" -o -name "*_wire.rs" \) 2>/dev/null || true)
  for f in $wire_files; do
    if grep -q "serde_json::to_vec\|serde_json::from_slice\|bincode::" "$f" 2>/dev/null; then
      grep -q "aura_core::util::serialization\|crate::util::serialization" "$f" 2>/dev/null || non_canonical+="$f"$'\n'
    fi
  done
  [[ -n "$non_canonical" ]] && emit_hits "Wire protocol without DAG-CBOR" "$non_canonical" || info "Wire protocols: canonical"

  local facts_files non_versioned=""
  facts_files=$(find crates -path "*/src/facts.rs" -type f 2>/dev/null | grep -v aura-core || true)
  for f in $facts_files; do
    if grep -q "Serialize\|Deserialize" "$f" 2>/dev/null; then
      grep -qE "aura_core::util::serialization|Versioned.*Fact|from_slice|to_vec" "$f" 2>/dev/null || non_versioned+="$f"$'\n'
    fi
  done
  [[ -n "$non_versioned" ]] && emit_hits "Facts without versioned serialization" "$non_versioned" || info "Facts: versioned"

  section "Wire protocol types — require DAG-CBOR tests"

  local using_serde_json=""
  local protocol_files
  protocol_files=$(rg -l "#\[derive.*Serialize.*Deserialize|#\[derive.*Deserialize.*Serialize" crates -g "protocol.rs" \
    | grep -Ev "/tests/|/benches/|/examples/" || true)

  for file in $protocol_files; do
    [[ -z "$file" ]] && continue
    if grep -qE "serde_json::(to_vec|from_slice|to_string|from_str)" "$file" 2>/dev/null; then
      if ! grep -qE "aura_core::util::serialization|serde_ipld_dagcbor" "$file" 2>/dev/null; then
        using_serde_json+="$file -- tests use serde_json but runtime uses DAG-CBOR"$'\n'
      fi
    fi
  done

  if [[ -n "$using_serde_json" ]]; then
    emit_hits "Wire protocol using wrong serialization" "$using_serde_json"
    hint "Replace serde_json with aura_core::util::serialization::{to_vec, from_slice} in tests"
  else
    info "Wire protocol types: serialization format consistent"
  fi
}

check_handler_hygiene() {
  section "Handler hygiene — stateless handlers; no bridge modules"

  local ceremony_services="ota_activation_service|recovery_service"
  local handler_state
  handler_state=$(rg --no-heading "Arc<.*(RwLock|Mutex)|RwLock<|Mutex<" crates/aura-agent/src/handlers -g "*.rs" \
    | grep -Ev "$ceremony_services" || true)
  emit_hits "Stateful handlers" "$handler_state"

  local bridge_files
  bridge_files=$(rg --files -g "*bridge*.rs" crates/aura-agent/src/handlers 2>/dev/null || true)
  emit_hits "Handler bridge modules" "$bridge_files"
}

check_serialization
check_handler_hygiene
