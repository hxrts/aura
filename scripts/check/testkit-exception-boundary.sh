#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowed_entries=(
  "crates/aura-testkit/src/flow_budget.rs|host-only deterministic budget test state"
  "crates/aura-testkit/src/handlers/memory/choreographic_memory.rs|stateful in-memory test handler"
  "crates/aura-testkit/src/handlers/mock.rs|stateful mock handler surface"
  "crates/aura-testkit/src/infrastructure/time.rs|deterministic native time fixture state"
  "crates/aura-testkit/src/mock_effects.rs|stateful mock effects surface"
  "crates/aura-testkit/src/mock_runtime_bridge.rs|native-only runtime bridge teardown bookkeeping"
  "crates/aura-testkit/src/stateful_effects/biometric.rs|stateful biometric test double"
  "crates/aura-testkit/src/stateful_effects/console.rs|stateful console test double"
  "crates/aura-testkit/src/stateful_effects/crypto.rs|stateful crypto test double"
  "crates/aura-testkit/src/stateful_effects/journal.rs|stateful journal test double"
  "crates/aura-testkit/src/stateful_effects/random.rs|stateful random test double"
  "crates/aura-testkit/src/stateful_effects/terminal.rs|stateful terminal test double"
  "crates/aura-testkit/src/stateful_effects/time.rs|stateful time test double"
  "crates/aura-testkit/src/stateful_effects/vm_bridge.rs|stateful VM bridge test double"
  "crates/aura-testkit/src/time/controllable_time.rs|controllable deterministic time source"
)

allowed_files=()
for entry in "${allowed_entries[@]}"; do
  path="${entry%%|*}"
  allowed_files+=("$path")
done

actual_files=()
while IFS= read -r path; do
  [[ -n "$path" ]] || continue
  actual_files+=("$path")
done < <(rg -l '^#!\[allow\(clippy::disallowed_types\)\]' crates/aura-testkit/src -g'*.rs' | sort)

sorted_allowed_files=()
while IFS= read -r path; do
  [[ -n "$path" ]] || continue
  sorted_allowed_files+=("$path")
done < <(printf '%s\n' "${allowed_files[@]}" | sort)

allowed_files=("${sorted_allowed_files[@]}")

if [[ "${actual_files[*]-}" != "${allowed_files[*]-}" ]]; then
  echo "testkit exception boundary: disallowed_types allowlist drift" >&2
  echo "expected:" >&2
  printf '  %s\n' "${allowed_files[@]}" >&2
  echo "actual:" >&2
  printf '  %s\n' "${actual_files[@]}" >&2
  exit 1
fi

lib_file="crates/aura-testkit/src/lib.rs"
assert_cfg_pair() {
  local target="$1"
  local count
  count="$(awk -v target="$target" '$0 == target { count++ } END { print count + 0 }' "$lib_file")"
  if [[ "$count" != "1" ]]; then
    echo "testkit exception boundary: expected exactly one \`$target\` entry in $lib_file" >&2
    exit 1
  fi
  if ! awk -v target="$target" '
    prev == "#[cfg(not(target_arch = \"wasm32\"))]" && $0 == target { found = 1 }
    { prev = $0 }
    END { exit(found ? 0 : 1) }
  ' "$lib_file"; then
    echo "testkit exception boundary: \`$target\` must be immediately preceded by \`#[cfg(not(target_arch = \"wasm32\"))]\` in $lib_file" >&2
    exit 1
  fi
}

assert_cfg_pair "pub mod mock_runtime_bridge;"
assert_cfg_pair "pub use mock_runtime_bridge::MockRuntimeBridge;"

echo "testkit exception boundary: clean (${#allowed_files[@]} named disallowed_types exceptions)"
