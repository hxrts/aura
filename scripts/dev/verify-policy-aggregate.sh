#!/usr/bin/env bash
# Verify existing canonical final gate evidence; never execute or replace it.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
root="$repo_root"
if [[ "${1:-}" == --root ]]; then root="${2:?missing root}"; shift 2; fi
[[ "${1:-}" == --receipt && $# == 2 ]] || exit 2
receipt="$2"
root="$(cd "$root" && pwd -P)"
[[ -f "$receipt" && ! -L "$receipt" &&
   "$receipt" == "$root/artifacts/disk-budget/receipts/"* ]] || exit 1
source "$repo_root/scripts/dev/build-receipt.sh"
[[ -z "$(git -C "$root" status --porcelain --untracked-files=normal)" ]] || exit 1
fingerprint="$(aura_receipt_source_fingerprint "$root")"
revision="$(git -C "$root" rev-parse HEAD)"
awk '
  /^ci-ownership-policy:$/ {getline; if ($0=="    just _policy-check check ownership-policy") found=1}
  END {exit !found}
' "$root/Justfile" || exit 1
awk '
  /^_policy-check \*ARGS:$/ {
    getline;
    if ($0=="    AURA_BUILD_WAIT_SECONDS=1800 bash scripts/dev/build-budget.sh --lane gates -- cargo run --quiet --manifest-path toolkit/xtask/Cargo.toml -- {{ ARGS }}") found=1
  }
  END {exit !found}
' "$root/Justfile" || exit 1
jq -e --arg root "$root" --arg fingerprint "$fingerprint" --arg revision "$revision" '
  .root==$root and .lane=="gates" and .source.revision==$revision and
  .source.fingerprint==$fingerprint and .admission.admitted==true and
  .command_argv==["cargo","run","--quiet","--manifest-path",
    "toolkit/xtask/Cargo.toml","--","check","ownership-policy"] and
  .command_exit_status==0 and .final_exit_status==0 and
  ([.phases[] | select(.phase=="command")] | length)==1 and
  ([.phases[] | .exit_status] | all(.==0))' "$receipt" >/dev/null || exit 1
printf 'policy aggregate: exact current-source terminal receipt=%s\n' "$receipt"
