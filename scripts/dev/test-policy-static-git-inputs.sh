#!/usr/bin/env bash
# Isolated static Git-selection contract; no Cargo or workspace build.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
source "$repo_root/scripts/dev/policy-dependency-inputs.sh"
source "$repo_root/scripts/dev/build-receipt.sh"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-static-git.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT
# This fixture owns its Git user configuration; ambient filters remain refused
# by production helpers and must not contaminate the isolated happy path.
mkdir -p "$fixture/home" "$fixture/xdg"
export HOME="$fixture/home" XDG_CONFIG_HOME="$fixture/xdg"
project="$fixture/project"
mkdir -p "$project"
project="$(cd "$project" && pwd -P)"
git init -q "$project"
printf 'one\n' > "$project/source.rs"
git -C "$project" add source.rs
git -C "$project" -c user.name=Fixture -c user.email=fixture@example.invalid \
  -c commit.gpgsign=false commit -qm initial
base="$(policy_static_git_identity "$project")"
[[ "$base" =~ ^[a-f0-9]{64}$ ]]
changed="$(AURA_UX_POLICY_DIFF_RANGE=HEAD policy_static_git_identity "$project")"
[[ "$base" != "$changed" ]] || { echo 'policy range was unbound' >&2; exit 1; }
changed="$(AURA_UX_GUIDANCE_CHANGED_FILES=source.rs policy_static_git_identity "$project")"
[[ "$base" != "$changed" ]] || { echo 'guidance override was unbound' >&2; exit 1; }
changed="$(AURA_UX_GUIDANCE_CHANGED_FILES='' policy_static_git_identity "$project")"
[[ "$base" != "$changed" ]] || {
  echo 'empty guidance override was conflated with unset' >&2; exit 1;
}
# A pull-request runner already exports GITHUB_BASE_REF=main. Compare controlled
# different values instead of accidentally requiring identical inputs to differ.
base_selector="$(GITHUB_BASE_REF='' policy_static_git_identity "$project")"
changed="$(GITHUB_BASE_REF=main policy_static_git_identity "$project")"
[[ "$base_selector" != "$changed" ]] || { echo 'Git base selector was unbound' >&2; exit 1; }
if AURA_UX_GUIDANCE_DIFF_RANGE=missing-revision \
    policy_static_git_identity "$project" user-flow-guidance-sync > "$fixture/output" 2>&1; then
  echo 'invalid guidance range was accepted' >&2; exit 1
fi
if AURA_UX_POLICY_DIFF_RANGE=--output=/tmp/escape \
    policy_static_git_identity "$project" user-flow-policy-guardrails > "$fixture/output" 2>&1; then
  echo 'option-like policy range was accepted' >&2; exit 1
fi
range_project="$fixture/range-project"
git init -q "$range_project"
range_project="$(cd "$range_project" && pwd -P)"
mkdir -p "$range_project/docs"
printf 'guide\n' > "$range_project/docs/guide.md"
git -C "$range_project" add docs/guide.md
git -C "$range_project" -c user.name=Fixture -c user.email=fixture@example.invalid \
  -c commit.gpgsign=false commit -qm initial
printf 'staged\n' > "$range_project/changed.rs"
git -C "$range_project" add changed.rs
blob="$(git -C "$range_project" rev-parse HEAD:docs/guide.md)"
git -C "$range_project" tag blob-ref "$blob"
[[ "$(git -C "$range_project" diff --name-only HEAD --)" == changed.rs ]] || {
  echo 'range fixture did not stage the changed file' >&2; exit 1;
}
for malformed_range in --stat docs blob-ref HEAD.. ...HEAD; do
  if AURA_UX_POLICY_DIFF_RANGE="$malformed_range" \
      policy_static_git_identity "$range_project" user-flow-policy-guardrails > "$fixture/output" 2>&1; then
    echo "malformed policy range was accepted: $malformed_range" >&2; exit 1
  fi
  if AURA_UX_GUIDANCE_DIFF_RANGE="$malformed_range" \
      policy_static_git_identity "$range_project" user-flow-guidance-sync > "$fixture/output" 2>&1; then
    echo "malformed guidance range was accepted: $malformed_range" >&2; exit 1
  fi
done
AURA_UX_POLICY_DIFF_RANGE=HEAD policy_static_git_identity "$range_project" \
  user-flow-policy-guardrails > /dev/null
AURA_UX_POLICY_DIFF_RANGE=HEAD...HEAD policy_static_git_identity "$range_project" \
  user-flow-policy-guardrails > /dev/null
AURA_UX_POLICY_DIFF_RANGE=HEAD..HEAD policy_static_git_identity "$range_project" \
  user-flow-policy-guardrails > /dev/null
mkdir -p "$fixture/home"
global_before="$(HOME="$fixture/home" policy_static_git_identity "$project")"
printf '[user]\nname = Fixture\n' > "$fixture/home/.gitconfig"
global_after="$(HOME="$fixture/home" policy_static_git_identity "$project")"
[[ "$global_before" != "$global_after" ]] || {
  echo 'global Git configuration was unbound' >&2; exit 1;
}
git -C "$project" config diff.fixture.command /bin/true
if policy_static_git_identity "$project" > "$fixture/output"; then
  echo 'external diff driver was accepted' >&2; exit 1
fi
git -C "$project" config --unset diff.fixture.command
git config --global filter.fixture.clean /bin/true
[[ "$(git config --global --get filter.fixture.clean)" == /bin/true ]]
global_filter_status=0
policy_static_git_identity "$project" > "$fixture/output" || global_filter_status=$?
[[ "$global_filter_status" == 2 && ! -s "$fixture/output" ]] || {
  echo 'global external Git filter did not produce its original refusal' >&2; exit 1;
}
git config --global --unset filter.fixture.clean
git -C "$project" config include.path "$fixture/other-config"
if policy_static_git_identity "$project" > "$fixture/output"; then
  echo 'external Git config include was accepted' >&2; exit 1
fi
git -C "$project" config --unset include.path
git -C "$project" branch test-base
after_ref="$(policy_static_git_identity "$project")"
[[ "$base" != "$after_ref" ]] || { echo 'Git ref map was unbound' >&2; exit 1; }
before_source="$(aura_receipt_source_fingerprint "$project")"
printf 'two\n' > "$project/source.rs"
[[ "$after_ref" == "$(policy_static_git_identity "$project")" ]] || {
  echo 'Git selector identity unexpectedly includes unstaged source' >&2; exit 1;
}
[[ "$before_source" != "$(aura_receipt_source_fingerprint "$project")" ]] || {
  echo 'unstaged source was unbound by source fingerprint' >&2; exit 1;
}
if GIT_INDEX_FILE="$fixture/foreign-index" policy_static_git_identity "$project" > "$fixture/output"; then
  echo 'alternate index was accepted' >&2; exit 1
fi
original="$(git -C "$project" rev-parse HEAD)"
git -C "$project" add source.rs
git -C "$project" -c user.name=Fixture -c user.email=fixture@example.invalid \
  -c commit.gpgsign=false commit -qm second
git -C "$project" replace "$original" HEAD
if policy_static_git_identity "$project" > "$fixture/output"; then
  echo 'replacement ref was accepted' >&2; exit 1
fi
git -C "$project" replace -d "$original"
linked="$fixture/linked"
git -C "$project" worktree add -qb linked "$linked"
linked="$(cd "$linked" && pwd -P)"
[[ "$(policy_static_git_identity "$linked")" =~ ^[a-f0-9]{64}$ ]] || {
  echo 'ordinary linked worktree refused' >&2; exit 1;
}
common_dir="$(git -C "$linked" rev-parse --path-format=absolute --git-common-dir)"
touch "$common_dir/shallow"
if policy_static_git_identity "$linked" > "$fixture/output"; then
  echo 'linked worktree common shallow accepted' >&2; exit 1
fi
rm "$common_dir/shallow"
for override in GIT_COMMON_DIR GIT_NAMESPACE GIT_SHALLOW_FILE GIT_CONFIG_PARAMETERS; do
  if env "$override=fixture" bash -c \
    'source "$1"; policy_static_git_identity "$2"' _ \
    "$repo_root/scripts/dev/policy-dependency-inputs.sh" "$project" > "$fixture/output" 2>&1; then
    echo "Git override accepted: $override" >&2; exit 1
  fi
done
printf 'policy static Git inputs: selectors, refs, source fingerprint and context refusals passed\n'
