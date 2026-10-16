#!/usr/bin/env bash
# One real toolkit compile receipt and a separate guarded static-check receipt.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
stage=packet
case "${1:-}" in
  --compile-stage) stage=compile; shift ;;
  --execution-stage) stage=execution; shift ;;
esac
root="$repo_root"
if [[ "${1:-}" == --root ]]; then root="${2:?missing root}"; shift 2; fi
compile_receipt=''
if [[ "${1:-}" == --compile-receipt ]]; then compile_receipt="${2:?missing receipt}"; shift 2; fi
[[ "${1:-}" == browser-driver-contract-sync && $# == 1 ]] || exit 2
check="$1"
root="$(cd "$root" && pwd -P)"
source "$repo_root/scripts/dev/policy-dependency-inputs.sh"
source "$repo_root/scripts/dev/build-receipt.sh"
scope=(crates/aura-web/src/harness/driver_contract.rs
  crates/aura-harness/playwright-driver/src/driver_contract.ts
  crates/aura-harness/playwright-driver/src/playwright_driver.ts)
if [[ "$stage" == packet ]]; then
  mkdir -p "$root/artifacts/disk-budget/receipts"
  umask 077
  if [[ -z "$compile_receipt" ]]; then
    packet="$(mktemp -d "$root/artifacts/disk-budget/receipts/policy-packet.XXXXXX")"
    chmod 700 "$packet"
    bash "$repo_root/scripts/dev/build-budget.sh" --root "$root" --lane policy-toolkit-compile -- \
      bash "$repo_root/scripts/dev/run-policy-static.sh" --compile-stage --root "$root" "$check" \
      > "$packet/compile.log" 2>&1 || { cat "$packet/compile.log"; exit 1; }
    cat "$packet/compile.log"
    compile_receipt="$(sed -n 's/^Build receipt: //p' "$packet/compile.log" | tail -1)"
    [[ -n "$compile_receipt" ]] || exit 1
  fi
  bash "$repo_root/scripts/dev/build-budget.sh" --root "$root" --lane policy-static-exec -- \
    bash "$repo_root/scripts/dev/run-policy-static.sh" --execution-stage --root "$root" \
      --compile-receipt "$compile_receipt" "$check"
  exit
fi
[[ "$stage" != execution || -n "$compile_receipt" ]] || exit 2
[[ "$stage" != compile || -z "$compile_receipt" ]] || exit 2
[[ -n "${AURA_BUILD_RECEIPT:-}" && -f "$AURA_BUILD_RECEIPT" ]] || exit 2
frozen="$(jq -r '.source.fingerprint // empty' "$AURA_BUILD_RECEIPT")"
[[ -n "$frozen" && "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || exit 1
toolkit_hash="$(policy_toolkit_inputs "$root")"
tool_hash="$(policy_tool_identity)"
binary="$root/toolkit/xtask/target/debug/aura-toolkit-xtask"
if [[ "$stage" == compile ]]; then
  aura_receipt_update --arg toolkit "$toolkit_hash" --arg tool "$tool_hash" \
    '.metadata.policy_toolkit={input_hash:$toolkit,tool_identity_hash:$tool}'
  bash "$repo_root/scripts/dev/build-phase.sh" compile -- \
    cargo build --manifest-path toolkit/xtask/Cargo.toml --bin aura-toolkit-xtask
  [[ -f "$binary" && -x "$binary" && ! -L "$binary" ]] || exit 1
  aura_receipt_update --arg hash "$(sha256sum "$binary" | awk '{print $1}')" \
    '.metadata.policy_toolkit.binary_hash=$hash'
  [[ "$toolkit_hash" == "$(policy_toolkit_inputs "$root")" &&
     "$tool_hash" == "$(policy_tool_identity)" &&
     "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || exit 1
  exit
fi
[[ -f "$compile_receipt" && ! -L "$compile_receipt" &&
   "$compile_receipt" == "$root/artifacts/disk-budget/receipts/"* ]] || exit 1
jq -e --arg root "$root" --arg toolkit "$toolkit_hash" --arg tool "$tool_hash" \
  '.root==$root and .lane=="policy-toolkit-compile" and .final_exit_status==0 and
   .metadata.policy_toolkit.input_hash==$toolkit and
   .metadata.policy_toolkit.tool_identity_hash==$tool and
   (.metadata.policy_toolkit.binary_hash|type)=="string" and
   ([.phases[] | select(.phase=="compile")] | length)==1 and
   ([.phases[] | .phase] | all(.=="compile" or .=="command")) and
   ([.phases[] | .exit_status] | all(.==0))' "$compile_receipt" >/dev/null || exit 1
[[ -f "$binary" && -x "$binary" && ! -L "$binary" &&
   "$(sha256sum "$binary" | awk '{print $1}')" == \
     "$(jq -r '.metadata.policy_toolkit.binary_hash' "$compile_receipt")" ]] || exit 1
scope_hash="$(policy_hash_files "$root" "${scope[@]}")"
scope_json="$(printf '%s\n' "${scope[@]}" | jq -Rs 'split("\n")|map(select(length>0))')"
AURA_RECEIPT_EVENT_KIND=dependency aura_receipt_update --arg check "$check" --argjson scope "$scope_json" \
  --arg scope_hash "$scope_hash" --arg toolkit "$toolkit_hash" --arg tool "$tool_hash" \
  --arg dependency "$compile_receipt" \
  --arg binary "$(jq -r '.metadata.policy_toolkit.binary_hash' "$compile_receipt")" \
  '.metadata.policy_static={check:$check,scope:$scope,scope_hash:$scope_hash,
    toolkit_hash:$toolkit,tool_identity_hash:$tool,
    compile_receipt:$dependency,binary_hash:$binary}'
bash "$repo_root/scripts/dev/build-phase.sh" execution -- "$binary" check "$check"
[[ "$scope_hash" == "$(policy_hash_files "$root" "${scope[@]}")" &&
   "$toolkit_hash" == "$(policy_toolkit_inputs "$root")" &&
   "$tool_hash" == "$(policy_tool_identity)" &&
   "$frozen" == "$(aura_receipt_source_fingerprint "$root")" &&
   "$(sha256sum "$binary" | awk '{print $1}')" == \
     "$(jq -r '.metadata.policy_static.binary_hash' "$AURA_BUILD_RECEIPT")" ]] || exit 1
