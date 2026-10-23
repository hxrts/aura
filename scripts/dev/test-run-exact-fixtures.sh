#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-exact-fixtures-test.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
trap 'rm -rf "$fixture"' EXIT
project="$fixture/project"
mkdir -p "$project/target/debug/deps" "$fixture/fakebin"
git init -q "$project"
printf '[package]\nname="fixture"\nversion="0.1.0"\n' > "$project/Cargo.toml"
printf '/target/\n/artifacts/\n' > "$project/.gitignore"
git -C "$project" add Cargo.toml .gitignore
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
cat > "$project/target/debug/deps/fixture-test" <<'EOF'
#!/usr/bin/env bash
name="$1"
if [[ "$*" == *--list* ]]; then
  [[ "${MISSING_NAME:-}" != "$name" ]] || exit 0
  if [[ "${MUTATE_NAME:-}" == "$name" ]]; then
    printf '\n# changed during discovery\n' >> "$PROJECT_ROOT/Cargo.toml"
  fi
  printf '%s: test\n' "$name"
elif [[ "${FAIL_NAME:-}" == "$name" ]]; then
  if [[ -n "${EXECUTION_COUNTER:-}" ]]; then
    printf '%s\n' "$name" >> "$EXECUTION_COUNTER"
    if [[ $(wc -l < "$EXECUTION_COUNTER" | tr -d ' ') == 2 ]]; then
      if [[ ${REPLAY_OUTCOME:-} == success ]]; then printf 'test %s ... ok\n' "$name"; exit 0; fi
      if [[ ${REPLAY_OUTCOME:-} == different ]]; then printf 'test %s ... FAILED\n' "$name"; exit 37; fi
    fi
  fi
  case ${REPLAY_TAMPER:-} in
    binary)
      printf '\n# changed original executable\n' >> "$0"
      digest=$(b3sum "$0")
      jq --arg digest "$digest" '.metadata.producer_binary_blake3=$digest' "$AURA_BUILD_RECEIPT" > "$AURA_BUILD_RECEIPT.changed"
      mv "$AURA_BUILD_RECEIPT.changed" "$AURA_BUILD_RECEIPT" ;;
    protected)
      file=$(jq -r .artifacts.protected_test_executable "$AURA_BUILD_RECEIPT")
      chmod u+w "$file"
      printf '\n# changed protected executable\n' >> "$file" ;;
    source) printf '\n# changed original source\n' >> "$PROJECT_ROOT/Cargo.toml" ;;
    inventory) file=$(jq -r .artifacts.expected_inventory "$AURA_BUILD_RECEIPT"); printf 'suite::foreign\n' > "$file" ;;
    config) jq '.metadata.features="foreign"' "$AURA_BUILD_RECEIPT" > "$AURA_BUILD_RECEIPT.changed"; mv "$AURA_BUILD_RECEIPT.changed" "$AURA_BUILD_RECEIPT" ;;
    helper) printf '\n# changed original phase helper\n' >> "$TEST_HELPER_FILE" ;;
  esac
  printf 'test %s ... FAILED\n' "$name"
  exit 101
else
  printf 'test %s ... ok\n' "$name"
fi
EOF
cat > "$fixture/fakebin/cargo" <<'EOF'
#!/usr/bin/env bash
[[ "$*" == *'--no-run --message-format=json'* ]] || exit 2
printf 'compile\n' >> "$COMPILE_COUNTER"
printf '{"reason":"compiler-artifact","executable":"%s/target/debug/deps/fixture-test","target":{"name":"smoke","kind":["test"],"test":true}}\n' "$PROJECT_ROOT"
printf '{"reason":"compiler-artifact","executable":"%s/target/debug/fixture-bin","target":{"name":"fixture-bin","kind":["bin"],"test":true}}\n' "$PROJECT_ROOT"
EOF
chmod +x "$project/target/debug/deps/fixture-test" "$fixture/fakebin/cargo"
export PATH="$fixture/fakebin:$PATH" PROJECT_ROOT="$project" COMPILE_COUNTER="$fixture/compile-count"
source "$repo_root/scripts/dev/build-receipt.sh"
export AURA_BUILD_PACKAGES=fixture AURA_BUILD_TEST_INVENTORY='suite::one,suite::two'
aura_receipt_init "$project" ownership-exact bash exact-fixtures
run_exact() {
  bash "${RUNNER_REPO_ROOT:-$repo_root}/scripts/dev/run-exact-fixtures.sh" --inside --root "$project" \
    --package fixture --test smoke --expected-count 2 \
    --name suite::one --name suite::two "$@"
}
run_exact > "$fixture/output"
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
jq -e '.metadata.packages=="fixture" and .metadata.expected_count==2 and
  .metadata.test_inventory=="suite::one,suite::two" and
  ([.phases[].phase]==["compile","discovery","discovery","execution","execution"]) and
  ([.phases[] | select(.phase=="discovery" or .phase=="execution") |
    select(.inventory_result=="matched" and .expected_count==1 and .observed_count==1)] | length)==4 and
  .artifacts.test_executable!=null' "$AURA_BUILD_RECEIPT" >/dev/null
if bash "$repo_root/scripts/dev/run-exact-fixtures.sh" --inside --root "$project" \
  --package fixture --test smoke --expected-count 2 --name suite::one --name suite::one \
  > "$fixture/output" 2>&1; then exit 1; fi
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
aura_receipt_init "$project" ownership-exact bash exact-fixtures
if MISSING_NAME=suite::two run_exact > "$fixture/output" 2>&1; then exit 1; fi
jq -e '([.phases[] | select(.phase=="discovery" and .inventory_result=="mismatch")] | length)==1 and
  ([.phases[] | select(.phase=="execution")] | length)==0' \
  "$AURA_BUILD_RECEIPT" >/dev/null
aura_receipt_init "$project" ownership-exact bash exact-fixtures
if FAIL_NAME=suite::one run_exact > "$fixture/output" 2>&1; then exit 1; fi
jq -e '([.phases[] | select(.phase=="execution" and .failed_count==1)] | length)==1 and
  ([.phases[] | select(.phase=="discovery" and .inventory_result=="matched")] | length)==2 and
  ([.phases[].phase]==["compile","discovery","discovery","execution"])' \
  "$AURA_BUILD_RECEIPT" >/dev/null
# Decisive-first is explicit: compile once, discover/execute the checkpoint,
# then retain complete adjacent discovery/execution and refusal obligations.
aura_receipt_init "$project" ownership-exact bash exact-decisive
before=$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')
run_exact --decisive-name suite::one > "$fixture/output"
[[ $(wc -l < "$COMPILE_COUNTER" | tr -d ' ') == $((before + 1)) ]]
jq -e '.metadata.decisive_name=="suite::one" and
  [.phases[].phase]==["compile","discovery","execution","discovery","execution"] and
  [.phases[]|select(.phase=="execution")|.expected_tests[0]]==["suite::one","suite::two"]' "$AURA_BUILD_RECEIPT" >/dev/null
aura_receipt_init "$project" ownership-exact bash exact-decisive-failure
if FAIL_NAME=suite::one run_exact --decisive-name suite::one > "$fixture/output" 2>&1; then exit 1; fi
jq -e '[.phases[].phase]==["compile","discovery","execution"] and .phases[-1].failed_count==1' "$AURA_BUILD_RECEIPT" >/dev/null
aura_receipt_init "$project" ownership-exact bash exact-decisive-missing-adjacent
if MISSING_NAME=suite::two run_exact --decisive-name suite::one > "$fixture/output" 2>&1; then exit 1; fi
jq -e '[.phases[].phase]==["compile","discovery","execution","discovery"] and
  .phases[-1].inventory_result=="mismatch" and
  ([.phases[]|select(.phase=="execution")]|length)==1' "$AURA_BUILD_RECEIPT" >/dev/null
aura_receipt_init "$project" ownership-exact bash exact-decisive-missing-checkpoint
if MISSING_NAME=suite::one run_exact --decisive-name suite::one > "$fixture/output" 2>&1; then exit 1; fi
jq -e '[.phases[].phase]==["compile","discovery"] and .phases[-1].inventory_result=="mismatch"' "$AURA_BUILD_RECEIPT" >/dev/null
cp "$project/Cargo.toml" "$fixture/decisive-original-Cargo.toml"
aura_receipt_init "$project" ownership-exact bash exact-decisive-source-change
if MUTATE_NAME=suite::one run_exact --decisive-name suite::one > "$fixture/output" 2>&1; then exit 1; fi
jq -e '[.phases[].phase]==["compile","discovery"]' "$AURA_BUILD_RECEIPT" >/dev/null
cp "$fixture/decisive-original-Cargo.toml" "$project/Cargo.toml"
for invalid in suite::two suite::absent; do
  before=$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')
  if run_exact --decisive-name "$invalid" > "$fixture/output" 2>&1; then exit 1; fi
  [[ $(wc -l < "$COMPILE_COUNTER" | tr -d ' ') == "$before" ]]
done
cp "$project/Cargo.toml" "$fixture/original-Cargo.toml"
aura_receipt_init "$project" ownership-exact bash exact-fixtures
if MUTATE_NAME=suite::one run_exact > "$fixture/output" 2>&1; then exit 1; fi
jq -e '([.phases[] | select(.phase=="discovery")] | length)==1 and
  ([.phases[] | select(.phase=="execution")] | length)==0' \
  "$AURA_BUILD_RECEIPT" >/dev/null
cp "$fixture/original-Cargo.toml" "$project/Cargo.toml"
aura_receipt_init "$project" ownership-exact bash exact-fixtures-timings
run_exact --timings > "$fixture/output"
jq -e '.phases[0].command_argv==["cargo","test","-p","fixture","--test","smoke","--timings","--no-run","--message-format=json"] and
  ([.phases[].phase]==["compile","discovery","discovery","execution","execution"])' \
  "$AURA_BUILD_RECEIPT" >/dev/null
echo 'exact fixtures: one compile, declared inventory, exact discovery/execution, duplicate/missing/failure refusal passed'

# Opt-in replay never compiles again, never executes another member after the
# original failure, and never promotes a passing/different replay to success.
cp "$project/target/debug/deps/fixture-test" "$fixture/original-executable"
export EXECUTION_COUNTER="$fixture/execution-count"
for outcome in same success different; do
  : > "$EXECUTION_COUNTER"
  aura_receipt_init "$project" ownership-exact bash replay-first-failure
  before=$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')
  actual=0
  FAIL_NAME=suite::one REPLAY_OUTCOME=$outcome run_exact --replay-first-failure --decisive-name suite::one > "$fixture/output" 2>&1 || actual=$?
  [[ $actual == 101 && $(wc -l < "$EXECUTION_COUNTER" | tr -d ' ') == 2 ]]
  [[ $(wc -l < "$COMPILE_COUNTER" | tr -d ' ') == $((before + 1)) ]]
  replay_status=101
  [[ $outcome != success ]] || replay_status=0
  [[ $outcome != different ]] || replay_status=37
  jq -e --argjson replay "$replay_status" '
    [.phases[].phase]==["compile","discovery","execution","execution"] and
    [.phases[]|select(.phase=="execution")|.command_exit_status]==[101,$replay] and
    .metadata.first_failure_replay=={test:"suite::one",original_exit_status:101,
      replay_exit_status:$replay,refusal_exit_status:null,post_replay_refusal_exit_status:null}' "$AURA_BUILD_RECEIPT" >/dev/null
  protected=$(jq -r .artifacts.protected_test_executable "$AURA_BUILD_RECEIPT")
  [[ -f "$protected" && -x "$protected" && ! -L "$protected" ]]
  [[ $(aura_receipt_stat_mode "$protected") == 500 ]]
  cmp "$project/target/debug/deps/fixture-test" "$protected"
done
# Normal discovery-first mode also replays only the first failed member.
: > "$EXECUTION_COUNTER"
aura_receipt_init "$project" ownership-exact bash replay-default-order
actual=0
FAIL_NAME=suite::one run_exact --replay-first-failure > "$fixture/output" 2>&1 || actual=$?
[[ $actual == 101 && $(wc -l < "$EXECUTION_COUNTER" | tr -d ' ') == 2 ]]
jq -e '[.phases[].phase]==["compile","discovery","discovery","execution","execution"]' "$AURA_BUILD_RECEIPT" >/dev/null
for tamper in binary protected source inventory config; do
  : > "$EXECUTION_COUNTER"
  aura_receipt_init "$project" ownership-exact bash replay-refusal
  actual=0
  FAIL_NAME=suite::one REPLAY_TAMPER=$tamper run_exact --replay-first-failure --decisive-name suite::one > "$fixture/output" 2>&1 || actual=$?
  [[ $actual == 101 && $(wc -l < "$EXECUTION_COUNTER" | tr -d ' ') == 1 ]]
  jq -e '[.phases[].phase]==["compile","discovery","execution"] and
    .metadata.first_failure_replay.original_exit_status==101 and
    .metadata.first_failure_replay.replay_exit_status==null and
    .metadata.first_failure_replay.refusal_exit_status==78' "$AURA_BUILD_RECEIPT" >/dev/null
  cp "$fixture/original-executable" "$project/target/debug/deps/fixture-test"
  cp "$fixture/original-Cargo.toml" "$project/Cargo.toml"
done
mkdir -p "$fixture/tooling/scripts/dev"
for helper in run-exact-fixtures.sh build-budget.sh build-phase.sh build-receipt.sh \
  libtest-inventory.awk scoped-builders.sh cache-rank.sh disk-report.sh policy-dependency-inputs.sh; do
  cp "$repo_root/scripts/dev/$helper" "$fixture/tooling/scripts/dev/$helper"
done
: > "$EXECUTION_COUNTER"
aura_receipt_init "$project" ownership-exact bash replay-helper-refusal
actual=0
FAIL_NAME=suite::one REPLAY_TAMPER=helper TEST_HELPER_FILE="$fixture/tooling/scripts/dev/build-phase.sh" \
  RUNNER_REPO_ROOT="$fixture/tooling" run_exact --replay-first-failure --decisive-name suite::one > "$fixture/output" 2>&1 || actual=$?
[[ $actual == 101 && $(wc -l < "$EXECUTION_COUNTER" | tr -d ' ') == 1 ]]
jq -e '.metadata.first_failure_replay.refusal_exit_status==78 and
  [.phases[].phase]==["compile","discovery","execution"]' "$AURA_BUILD_RECEIPT" >/dev/null
aura_receipt_init "$project" ownership-exact bash replay-enabled-success
run_exact --replay-first-failure > "$fixture/output"
jq -e '[.phases[].phase]==["compile","discovery","discovery","execution","execution"] and
  .metadata.first_failure_replay==null' "$AURA_BUILD_RECEIPT" >/dev/null
echo 'exact fixtures: opt-in original failure/replay outcomes, original identity refusals and unchanged success passed'
