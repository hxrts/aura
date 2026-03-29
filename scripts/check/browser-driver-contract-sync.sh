#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "browser-driver-contract-sync: $*" >&2
  exit 1
}

rust_contract="crates/aura-web/src/harness/driver_contract.rs"
ts_contract="crates/aura-harness/playwright-driver/src/driver_contract.ts"
ts_driver="crates/aura-harness/playwright-driver/src/playwright_driver.ts"

[[ -f "$rust_contract" ]] || fail "missing Rust contract file: $rust_contract"
[[ -f "$ts_contract" ]] || fail "missing TS contract file: $ts_contract"
[[ -f "$ts_driver" ]] || fail "missing TS driver file: $ts_driver"

mapfile -t rust_consts < <(
  perl -0ne 'while (/pub\(crate\) const (\w+): &str =\s*"([^"]+)";/sg) { print "$1=$2\n"; }' "$rust_contract"
)
mapfile -t ts_consts < <(
  perl -0ne 'while (/export const (\w+) =\s*"([^"]+)";/sg) { print "$1=$2\n"; }' "$ts_contract"
)

[[ "${#rust_consts[@]}" -gt 0 ]] || fail "failed to extract Rust contract constants"
[[ "${#ts_consts[@]}" -gt 0 ]] || fail "failed to extract TS contract constants"

if [[ "$(printf '%s\n' "${rust_consts[@]}" | sort)" != "$(printf '%s\n' "${ts_consts[@]}" | sort)" ]]; then
  echo "Rust constants:" >&2
  printf '%s\n' "${rust_consts[@]}" | sort >&2
  echo "TS constants:" >&2
  printf '%s\n' "${ts_consts[@]}" | sort >&2
  fail "Rust and TS browser-driver contract constants differ"
fi

mapfile -t rust_semantic_fields < <(
  perl -0ne 'if (/struct SemanticQueuePayload \{(.*?)\}/s) { while ($1 =~ /pub\(crate\) (\w+):/g) { print "$1\n"; } }' "$rust_contract"
)
mapfile -t rust_runtime_fields < <(
  perl -0ne 'if (/struct RuntimeStageQueuePayload \{(.*?)\}/s) { while ($1 =~ /pub\(crate\) (\w+):/g) { print "$1\n"; } }' "$rust_contract"
)
mapfile -t ts_semantic_fields < <(
  perl -0ne 'if (/type SemanticQueuePayload = \{(.*?)\};/s) { while ($1 =~ /(\w+):/g) { print "$1\n"; } }' "$ts_contract"
)
mapfile -t ts_runtime_fields < <(
  perl -0ne 'if (/type RuntimeStageQueuePayload = \{(.*?)\};/s) { while ($1 =~ /(\w+):/g) { print "$1\n"; } }' "$ts_contract"
)

if [[ "$(printf '%s\n' "${rust_semantic_fields[@]}" | sort)" != "$(printf '%s\n' "${ts_semantic_fields[@]}" | sort)" ]]; then
  fail "Semantic queue payload fields differ between Rust and TS contracts"
fi
if [[ "$(printf '%s\n' "${rust_runtime_fields[@]}" | sort)" != "$(printf '%s\n' "${ts_runtime_fields[@]}" | sort)" ]]; then
  fail "Runtime-stage queue payload fields differ between Rust and TS contracts"
fi

mapfile -t raw_driver_names < <(
  perl -0ne 'while (/"(__AURA_DRIVER_[A-Z0-9_]+)"/g) { print "$1\n"; }' "$ts_driver" | sort -u
)
mapfile -t allowed_driver_names < <(
  printf '%s\n' \
    "__AURA_DRIVER_OBSERVER_INSTALLED" \
    "__AURA_DRIVER_PUSH_CLIPBOARD" \
    "__AURA_DRIVER_PUSH_RENDER_HEARTBEAT" \
    "__AURA_DRIVER_PUSH_STATE" \
    "__AURA_DRIVER_PUSH_UI_STATE" \
    "__AURA_DRIVER_RUNTIME_STAGE_ENQUEUE_DISPATCHED__" \
    "__AURA_DRIVER_SEMANTIC_ENQUEUE_DISPATCHED__" \
    | sort -u
)

unexpected_driver_names="$(
  comm -23 \
    <(printf '%s\n' "${raw_driver_names[@]}") \
    <(
      {
        printf '%s\n' "${allowed_driver_names[@]}"
        printf '%s\n' "${ts_consts[@]}" | cut -d= -f2
      } | sort -u
    )
)"
if [[ -n "$unexpected_driver_names" ]]; then
  echo "Unexpected raw TS driver names:" >&2
  printf '%s\n' "$unexpected_driver_names" >&2
  fail "TS driver contains raw __AURA_DRIVER_* literals outside the sanctioned contract set"
fi

if ! perl -0ne 'exit 1 if /JSON\.stringify\(\{\s*command_id:\s*commandId,\s*request_json:\s*requestJson,\s*\}\)/s' "$ts_driver"; then
  fail "TS driver still hand-builds semantic queue payload JSON"
fi

if ! perl -0ne 'exit 1 if /JSON\.stringify\(\{\s*command_id:\s*commandId,\s*runtime_identity_json:\s*runtimeIdentityJson,\s*\}\)/s' "$ts_driver"; then
  fail "TS driver still hand-builds runtime-stage queue payload JSON"
fi

echo "browser-driver-contract-sync: clean"
