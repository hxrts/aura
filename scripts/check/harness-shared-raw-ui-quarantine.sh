#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "harness-shared-raw-ui-quarantine: $*" >&2
  exit 1
}

semantic_fn="$(perl -0ne 'print $1 if /fn execute_semantic_shared_step\(.*?\) -> Result<\(\)> \{(.*?)\n\}\n\nfn unsatisfied_action_preconditions/s' crates/aura-harness/src/executor.rs)"
[[ -n "$semantic_fn" ]] || fail "could not extract execute_semantic_shared_step"

for forbidden in 'ToolRequest::ClickButton' 'ToolRequest::FillInput' 'ToolRequest::FillField' '.click_button(' '.fill_input(' '.fill_field(' '.click_target('; do
  if grep -Fq "$forbidden" <<<"$semantic_fn"; then
    fail "shared semantic execution still reaches raw helper: $forbidden"
  fi
done

echo "harness shared raw-ui quarantine: clean"
