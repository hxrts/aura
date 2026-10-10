#!/usr/bin/env bash
# Execute only on supported Linux, from a clean committed candidate.
set -euo pipefail
export NO_COLOR=1
root=$(git rev-parse --show-toplevel)
export AURA_KANI_ROOT=${AURA_KANI_ROOT:-$root/.tmp/kani-root}
[[ $(uname -s) == Linux ]] || { printf 'Kani sensitivity requires supported Linux execution\n' >&2; exit 2; }
[[ -z $(git -C "$root" status --porcelain) ]] || { printf 'A clean committed candidate is required\n' >&2; exit 2; }
artifact_dir=${AURA_KANI_SENSITIVITY_ARTIFACTS:?Set an absolute artifact directory outside disposable source copies}
[[ $artifact_dir == /* ]] || exit 2
mkdir -p "$artifact_dir"
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-kani-sensitivity.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
for mutation in threshold terminal; do
  source_dir="$fixture/$mutation"
  mkdir -p "$source_dir"
  git -C "$root" archive HEAD | tar -x -C "$source_dir"
  decision="$source_dir/crates/aura-consensus/src/core/decision.rs"
  if [[ $mutation == threshold ]]; then
    perl -0pi -e '$n=s/count >= threshold && count > best_count/count > threshold \&\& count > best_count/g; die "threshold anchor mismatch" unless $n==1' "$decision"
    harness=threshold_met_matches_reference
    property='threshold matches independent reference'
  else
    perl -0pi -e '$n=s/ConsensusPhase::FastPathActive \| ConsensusPhase::FallbackActive\n    \) \{/ConsensusPhase::FastPathActive | ConsensusPhase::FallbackActive | ConsensusPhase::Committed\n    ) {/g; die "terminal anchor mismatch" unless $n==1' "$decision"
    harness=committed_state_is_terminal
    property='committed rejects shares'
  fi
  log="$artifact_dir/$mutation.log"
  result=0
  (
    cd "$source_dir"
    CARGO_BUILD_JOBS=4 nix develop --command bash scripts/dev/build-budget.sh --lane "kani-sensitivity-$mutation" -- \
      just _run-kani cargo kani --package hxrts-aura-consensus --harness "$harness" --output-format regular
  ) > "$log" 2>&1 || result=$?
  [[ $result -ne 0 ]] || { printf 'Mutation unexpectedly verified: %s\n' "$mutation" >&2; exit 1; }
  rg -F 'VERIFICATION:- FAILED' "$log" >/dev/null || { printf 'No actual verification failure: %s\n' "$log" >&2; exit 1; }
  rg -Fx "Failed Checks: $property" "$log" >/dev/null || { printf 'Required property counterexample absent: %s\n' "$log" >&2; exit 1; }
  printf '%s: actual required-property counterexample retained in %s\n' "$mutation" "$log"
done
