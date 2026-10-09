#!/usr/bin/env bash
# Isolated real-Git fixture with mocked Cargo/build admission; no compiler.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$here/../.." && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir "$fixture/bin"
export REAL_GIT
REAL_GIT=$(command -v git)
export CASE_LOG="$fixture/calls.jsonl" CASE_BUDGET="$fixture/budget.log"
export AURA_BUILD_WAIT_SECONDS=0 CARGO_TARGET_DIR=must-not-be-inherited
export FAIL_PHASE='' FAIL_STATUS=0
cat > "$fixture/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ ! ${CARGO_TARGET_DIR+x} && "$CARGO_BUILD_JOBS" == 4 ]]
phase=$1; shift
manifest='' package='' locked=false
while (( $# )); do
  case "$1" in
    --manifest-path) manifest=$2; shift 2;;
    --package) package=$2; shift 2;;
    --locked) locked=true; shift;;
    *) shift;;
  esac
done
[[ -f "$manifest" ]]
[[ "$phase" == fmt || "$locked" == true ]]
if [[ "$phase" == metadata ]]; then
  package=$(awk '/^name = / {gsub(/"/, "", $3); print $3; exit}' "$manifest")
fi
jq -cn --arg phase "$phase" --arg manifest "$manifest" --arg package "$package" \
  '{phase:$phase,manifest:$manifest,package:$package}' >> "$CASE_LOG"
[[ "$phase" != "$FAIL_PHASE" ]] || exit "$FAIL_STATUS"
if [[ "$phase" == metadata ]]; then
  jq -cn --arg manifest "$manifest" --arg package "$package" \
    '{packages:[{name:"foreign-owner",manifest_path:"/foreign/Cargo.toml"},{name:$package,manifest_path:$manifest}]}'
fi
MOCK
chmod +x "$fixture/bin/cargo"
export PATH="$fixture/bin:$PATH"
new_case() {
  repo="$fixture/$1"
  mkdir -p "$repo/scripts/dev"
  git -C "$repo" init -q
  git -C "$repo" config core.hooksPath "$fixture/no-hooks"
  cp "$here/check-staged-rust.sh" "$repo/scripts/dev/check-staged-rust.sh"
  cat > "$repo/scripts/dev/build-budget.sh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == --lane && "$2" == pre-commit && "$3" == -- ]]
[[ "$AURA_BUILD_WAIT_SECONDS" == 0 ]]
shift 3
echo admitted >> "$CASE_BUDGET"
[[ "$FAIL_PHASE" != budget ]] || exit "$FAIL_STATUS"
exec "$@"
MOCK
  : > "$CASE_LOG"; : > "$CASE_BUDGET"
  FAIL_PHASE='' FAIL_STATUS=0
}
package_file() {
  local path=$1 package=$2 file=$3
  mkdir -p "$repo/$path/$(dirname "$file")"
  printf '[package]\nname = "%s"\nversion = "0.1.0"\n' "$package" > "$repo/$path/Cargo.toml"
  printf 'fn fixture() {}\n' > "$repo/$path/$file"
  git -C "$repo" add -- "$path/$file"
}
run_case() {
  status=0
  (cd "$repo"; bash "$repo_root/.githooks/pre-commit") > "$fixture/output" 2>&1 || status=$?
}
expect_checks() {
  [[ "$status" == 0 ]]
  jq -es --argjson count "$1" 'length == $count*3 and ([.[]|select(.phase=="check")]|length)==$count and all(.[];.package!="foreign-owner")' "$CASE_LOG" >/dev/null
  [[ $(wc -l < "$CASE_BUDGET" | tr -d ' ') == "$1" ]]
}
new_case root
package_file crates/aura-app hxrts-aura-app src/lib.rs
run_case; expect_checks 1
new_case nested
package_file 'crates/space crate' hxrts-aura-app src/workflows/deep/action.rs
package_file 'crates/space crate' hxrts-aura-app $'src/line\nbreak.rs'
run_case; expect_checks 1
new_case multiple
package_file crates/aura-app hxrts-aura-app src/workflows/action.rs
package_file crates/aura-terminal aura-terminal src/command/execute.rs
run_case; expect_checks 2
new_case excluded-toolkit
package_file toolkit/xtask aura-toolkit-xtask src/checks/policy.rs
run_case; expect_checks 1
new_case cross-package-rename
package_file crates/aura-app hxrts-aura-app src/moved.rs
package_file crates/aura-terminal aura-terminal src/kept.rs
git -C "$repo" -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm baseline
git -C "$repo" mv crates/aura-app/src/moved.rs crates/aura-terminal/src/moved.rs
run_case; expect_checks 2
new_case deletion
package_file crates/aura-app hxrts-aura-app src/workflows/deep/action.rs
git -C "$repo" -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm baseline
git -C "$repo" rm -q -- crates/aura-app/src/workflows/deep/action.rs
run_case; expect_checks 1
new_case non-rust
printf 'fixture\n' > "$repo/README.md"
git -C "$repo" add README.md
run_case; expect_checks 0
for phase in metadata fmt check budget; do
  new_case "failure-$phase"
  package_file crates/aura-app hxrts-aura-app src/workflows/action.rs
  FAIL_PHASE=$phase FAIL_STATUS=37
  run_case
  [[ "$status" == 37 ]] || { cat "$fixture/output"; exit 1; }
  if [[ "$phase" != check && "$phase" != budget ]]; then [[ ! -s "$CASE_BUDGET" ]]; fi
done
new_case unowned
printf 'fn fixture() {}\n' > "$repo/unowned.rs"
git -C "$repo" add unowned.rs
run_case
[[ "$status" != 0 && ! -s "$CASE_BUDGET" ]]
new_case git-failure
package_file crates/aura-app hxrts-aura-app src/workflows/action.rs
cat > "$fixture/bin/git" <<'MOCK'
#!/usr/bin/env bash
[[ "${1:-}" != diff ]] || exit 38
exec "$REAL_GIT" "$@"
MOCK
chmod +x "$fixture/bin/git"
run_case
[[ "$status" == 38 && ! -s "$CASE_LOG" && ! -s "$CASE_BUDGET" ]]
rm "$fixture/bin/git"
new_case ignored-staged
printf 'ignored.rs\n' > "$repo/.gitignore"
printf 'fn fixture() {}\n' > "$repo/ignored.rs"
git -C "$repo" add -f ignored.rs
run_case
[[ "$status" == 1 && ! -s "$CASE_LOG" && ! -s "$CASE_BUDGET" ]]
new_case ignored-newline
printf 'ab?cd\n' > "$repo/.gitignore"
printf fixture > "$repo/"$'ab\ncd'
git -C "$repo" add -f -- $'ab\ncd'
run_case
[[ "$status" == 1 && ! -s "$CASE_LOG" && ! -s "$CASE_BUDGET" ]]
echo 'staged Rust package fixtures passed (15 cases, no compiler)'
