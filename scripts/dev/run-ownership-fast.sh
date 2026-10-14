#!/usr/bin/env bash
# Source-scoped ownership lint preflight. The aggregate ownership gate remains
# the integration authority; this lane gives fast feedback on edited files.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
inside=0
if [[ "${1:-}" == --inside ]]; then inside=1; shift; fi
root="$repo_root"
if [[ "${1:-}" == --root ]]; then root="${2:?missing root}"; shift 2; fi
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
if (( inside == 0 )); then
  export AURA_BUILD_PACKAGES=hxrts-aura-macros
  exec bash "$repo_root/scripts/dev/build-budget.sh" --root "$root" --lane ownership-fast -- \
    bash "$repo_root/scripts/dev/run-ownership-fast.sh" --inside --root "$root" "${paths[@]}"
fi
[[ -n "${AURA_BUILD_RECEIPT:-}" && -f "$AURA_BUILD_RECEIPT" ]] || {
  echo 'run-ownership-fast: original budget receipt required' >&2; exit 2;
}
source "$repo_root/scripts/dev/build-receipt.sh"
frozen="$(jq -r '.source.fingerprint // empty' "$AURA_BUILD_RECEIPT")"
[[ -n "$frozen" && "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
  echo 'run-ownership-fast: source changed before lint compile' >&2; exit 1;
}
modes=(actor-owned-task-spawn semantic-owner-no-spawn optional-owner-boundary
  authoritative-ref-no-reresolution weak-to-strong-identifier-upgrade time-domain-usage)
scope_json="$(printf '%s\n' "${paths[@]}" | jq -Rs 'split("\n") | map(select(length>0))')"
modes_json="$(printf '%s\n' "${modes[@]}" | jq -Rs 'split("\n") | map(select(length>0))')"
aura_receipt_update --argjson scope "$scope_json" --argjson modes "$modes_json" \
  '.metadata.source_scope=$scope | .metadata.policy_modes=$modes'
bash "$repo_root/scripts/dev/build-phase.sh" compile -- \
  cargo build -p hxrts-aura-macros --bin ownership_lints
lint_binary="$root/target/debug/ownership_lints"
[[ -f "$lint_binary" && -x "$lint_binary" && ! -L "$lint_binary" ]] || {
  echo 'run-ownership-fast: compiled ownership lint unavailable' >&2; exit 1;
}
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
