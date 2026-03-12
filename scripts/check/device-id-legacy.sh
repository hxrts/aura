#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

mode="${1:-legacy}"
violations=0

check_pattern() {
  local description="$1"
  local pattern="$2"
  local extra_args=("${@:3}")
  local output

  output="$(rg -n --glob '*.rs' "${extra_args[@]}" "$pattern" crates || true)"
  if [[ -n "$output" ]]; then
    echo "✖ $description"
    echo "$output"
    echo
    violations=$((violations + 1))
  fi
}

case "$mode" in
  legacy)
    check_pattern \
      "legacy authority-from-device UUID coercion detected" \
      'AuthorityId::from_uuid\(([^)]*device[^)]*)\)|AuthorityId\([^)]*device[^)]*\)'

    check_pattern \
      "legacy authority-from-device field coercion detected" \
      'AuthorityId::from_uuid\(([^)]*(device|participant)[^)]*\.0[^)]*)\)'

    check_pattern \
      "legacy device-from-authority UUID coercion detected" \
      'DeviceId::from_uuid\(([^)]*authority[^)]*)\)|DeviceId\([^)]*authority[^)]*\)'

    if [[ "$violations" -ne 0 ]]; then
      echo "device-id-legacy: found $violations legacy authority/device coercion pattern(s)"
      echo "use AuthorityId::for_device(...), DeviceId::for_authority(...), or explicit authority-aware role constructors instead"
      exit 1
    fi

    echo "device-id-legacy: clean"
    ;;
  audit-live)
    live_globs=(
      --glob '!**/tests/**'
      --glob '!**/test_*.rs'
      --glob '!**/*_test.rs'
    )

    check_pattern \
      "live authority/device helper derivation detected" \
      '^(?!\s*//).*(AuthorityId::for_device\(|DeviceId::for_authority\()' \
      -P \
      "${live_globs[@]}"

    check_pattern \
      "live bootstrap authority derivation helper detected" \
      '^(?!\s*//).*(derive_authority_id\()' \
      -P \
      "${live_globs[@]}"

    if [[ "$violations" -ne 0 ]]; then
      echo "device-id-legacy audit: found $violations live authority/device derivation pattern(s)"
      echo "these are explicit helper-based derivations, not raw UUID coercions"
      exit 1
    fi

    echo "device-id-legacy audit: clean"
    ;;
  audit-runtime)
    runtime_globs=(
      --glob '!**/tests/**'
      --glob '!**/test_*.rs'
      --glob '!**/*_test.rs'
      --glob '!crates/aura-agent/src/runtime/effects.rs'
      --glob '!crates/aura-agent/src/handlers/sessions/coordination.rs'
      --glob '!crates/aura-simulator/src/choreography_transport.rs'
      --glob '!crates/aura-simulator/src/testkit_bridge.rs'
    )

    check_pattern \
      "runtime authority/device helper derivation detected" \
      '^(?!\s*//).*(AuthorityId::for_device\(|DeviceId::for_authority\()' \
      -P \
      "${runtime_globs[@]}"

    check_pattern \
      "runtime bootstrap authority derivation helper detected" \
      '^(?!\s*//).*(derive_authority_id\()' \
      -P \
      "${runtime_globs[@]}"

    if [[ "$violations" -ne 0 ]]; then
      echo "device-id-legacy runtime audit: found $violations runtime authority/device derivation pattern(s)"
      exit 1
    fi

    echo "device-id-legacy runtime audit: clean"
    ;;
  *)
    echo "usage: $0 [legacy|audit-live|audit-runtime]" >&2
    exit 2
    ;;
esac
