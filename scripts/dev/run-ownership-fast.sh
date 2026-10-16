#!/usr/bin/env bash
# Source-scoped ownership lint preflight. The aggregate ownership gate remains
# the integration authority; this lane gives fast feedback on edited files.
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
if [[ "${1:-}" == --compile-receipt ]]; then compile_receipt="${2:?missing compile receipt}"; shift 2; fi
[[ $# -gt 0 ]] || { echo 'run-ownership-fast: at least one crate Rust file required' >&2; exit 2; }
root="$(cd "$root" && pwd -P)"
paths=()
for relative in "$@"; do
  [[ "$relative" =~ ^crates/[A-Za-z0-9_./-]+\.rs$ && "$relative" != *../* ]] || {
    echo 'run-ownership-fast: scope must be crate Rust paths' >&2; exit 2;
  }
  path="$root/$relative"
  [[ -f "$path" && ! -L "$path" ]] || { echo 'run-ownership-fast: missing or linked source' >&2; exit 2; }
  resolved="$(cd "$(dirname "$path")" && pwd -P)/$(basename "$path")"
  [[ "$resolved" == "$root/crates/"* ]] || { echo 'run-ownership-fast: source escaped checkout' >&2; exit 2; }
  paths+=("$relative")
done
[[ "$(printf '%s\n' "${paths[@]}" | sort -u | wc -l | tr -d ' ')" == "${#paths[@]}" ]] || {
  echo 'run-ownership-fast: duplicate source path' >&2; exit 2;
}
if [[ "$stage" == packet ]]; then
  mkdir -p "$root/artifacts/disk-budget/receipts"
  umask 077
  if [[ -z "$compile_receipt" ]]; then
  packet="$(mktemp -d "$root/artifacts/disk-budget/receipts/policy-packet.XXXXXX")"
  chmod 700 "$packet"
  export AURA_BUILD_PACKAGES=hxrts-aura-macros
  bash "$repo_root/scripts/dev/build-budget.sh" --root "$root" --lane ownership-lint-compile -- \
    bash "$repo_root/scripts/dev/run-ownership-fast.sh" --compile-stage --root "$root" \
      "${paths[@]}" > "$packet/compile.log" 2>&1 || {
    cat "$packet/compile.log"; exit 1;
  }
  cat "$packet/compile.log"
  compile_receipt="$(sed -n 's/^Build receipt: //p' "$packet/compile.log" | tail -1)"
  [[ -n "$compile_receipt" ]] || { echo 'run-ownership-fast: compile receipt absent' >&2; exit 1; }
  fi
  bash "$repo_root/scripts/dev/build-budget.sh" --root "$root" --lane ownership-lint-exec -- \
    bash "$repo_root/scripts/dev/run-ownership-fast.sh" --execution-stage --root "$root" \
      --compile-receipt "$compile_receipt" "${paths[@]}"
  exit
fi
[[ "$stage" != execution || -n "$compile_receipt" ]] || {
  echo 'run-ownership-fast: execution requires original compile receipt' >&2; exit 2;
}
[[ "$stage" != compile || -z "$compile_receipt" ]] || exit 2
[[ -n "${AURA_BUILD_RECEIPT:-}" && -f "$AURA_BUILD_RECEIPT" ]] || {
  echo 'run-ownership-fast: original budget receipt required' >&2; exit 2;
}
source "$repo_root/scripts/dev/build-receipt.sh"
source "$repo_root/scripts/dev/policy-dependency-inputs.sh"
frozen="$(jq -r '.source.fingerprint // empty' "$AURA_BUILD_RECEIPT")"
[[ -n "$frozen" && "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
  echo 'run-ownership-fast: source changed before lint compile' >&2; exit 1;
}
modes=(actor-owned-task-spawn semantic-owner-no-spawn optional-owner-boundary
  authoritative-ref-no-reresolution weak-to-strong-identifier-upgrade time-domain-usage)
scope_json="$(printf '%s\n' "${paths[@]}" | jq -Rs 'split("\n") | map(select(length>0))')"
modes_json="$(printf '%s\n' "${modes[@]}" | jq -Rs 'split("\n") | map(select(length>0))')"
scope_hash="$(policy_hash_files "$root" "${paths[@]}")"
tool_hash="$(policy_tool_identity)"
aura_receipt_update --argjson scope "$scope_json" --argjson modes "$modes_json" \
  --arg scope_hash "$scope_hash" --arg tool_hash "$tool_hash" \
  '.metadata.source_scope=$scope | .metadata.policy_modes=$modes |
   .metadata.policy_scope_hash=$scope_hash | .metadata.policy_tool_identity_hash=$tool_hash'
if [[ "$stage" == execution ]]; then
  [[ -f "$compile_receipt" && ! -L "$compile_receipt" &&
     "$compile_receipt" == "$root/artifacts/disk-budget/receipts/"* ]] || {
    echo 'run-ownership-fast: original compile receipt missing or outside checkout' >&2; exit 1;
  }
  jq -e --arg root "$root" --arg source "$frozen" --arg scope "$scope_hash" \
    --arg tool "$tool_hash" --argjson paths "$scope_json" \
    '.root==$root and .lane=="ownership-lint-compile" and
     .source.fingerprint==$source and .final_exit_status==0 and
     .metadata.source_scope==$paths and .metadata.policy_scope_hash==$scope and
     .metadata.policy_tool_identity_hash==$tool and
     ([.phases[] | select(.phase=="compile")] | length)==1 and
     ([.phases[] | .phase] | all(.=="compile" or .=="command")) and
     ([.phases[] | .exit_status] | all(.==0)) and
     (.metadata.policy_binary_hash|type)=="string"' "$compile_receipt" >/dev/null || {
    echo 'run-ownership-fast: compile dependency failed or stale' >&2; exit 1;
  }
  lint_binary="$root/target/debug/ownership_lints"
  [[ -f "$lint_binary" && -x "$lint_binary" && ! -L "$lint_binary" &&
     "$(sha256sum "$lint_binary" | awk '{print $1}')" == \
       "$(jq -r '.metadata.policy_binary_hash' "$compile_receipt")" ]] || {
    echo 'run-ownership-fast: original compiled lint artifact changed' >&2; exit 1;
  }
  AURA_RECEIPT_EVENT_KIND=dependency aura_receipt_update --arg dependency "$compile_receipt" \
    --arg binary "$(jq -r '.metadata.policy_binary_hash' "$compile_receipt")" \
    '.metadata.policy_compile_receipt=$dependency | .metadata.policy_binary_hash=$binary'
else
bash "$repo_root/scripts/dev/build-phase.sh" compile -- \
  cargo build -p hxrts-aura-macros --bin ownership_lints
lint_binary="$root/target/debug/ownership_lints"
[[ -f "$lint_binary" && -x "$lint_binary" && ! -L "$lint_binary" ]] || {
  echo 'run-ownership-fast: compiled ownership lint unavailable' >&2; exit 1;
}
aura_receipt_update --arg hash "$(sha256sum "$lint_binary" | awk '{print $1}')" \
  '.metadata.policy_binary_hash=$hash'
[[ "$stage" != compile ]] || exit 0
fi
absolute=()
for relative in "${paths[@]}"; do absolute+=("$root/$relative"); done
for mode in "${modes[@]}"; do
  [[ "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
    echo 'run-ownership-fast: source changed during batch' >&2; exit 1;
  }
  bash "$repo_root/scripts/dev/build-phase.sh" execution -- \
    "$lint_binary" "$mode" "${absolute[@]}"
done
[[ "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
  echo 'run-ownership-fast: source changed during batch' >&2; exit 1;
}
[[ "$scope_hash" == "$(policy_hash_files "$root" "${paths[@]}")" &&
   "$tool_hash" == "$(policy_tool_identity)" &&
   "$(sha256sum "$lint_binary" | awk '{print $1}')" == \
     "$(jq -r '.metadata.policy_binary_hash' "$AURA_BUILD_RECEIPT")" ]] || {
  echo 'run-ownership-fast: source, tool, or compiled lint changed during batch' >&2; exit 1;
}
