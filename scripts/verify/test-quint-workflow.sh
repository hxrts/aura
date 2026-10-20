#!/usr/bin/env bash
# Contract fixtures use fake commands; they establish wrapper behavior, not proofs.
set -euo pipefail
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-quint-contract.XXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
mkdir -p "$fixture/repo/scripts/verify" "$fixture/bin" "$fixture/tmp"
cp "$script_dir/"{quint-workflow,quint-trace,quint-observation,workflow}.sh "$fixture/repo/scripts/verify/"
for path in verification/quint/root.qnt verification/quint/amp/nested.qnt verification/quint/consensus/core.qnt verification/quint/consensus/adversary.qnt verification/quint/consensus/liveness.qnt crates/aura-simulator/tests/quint_specs/simulation.qnt; do
    mkdir -p "$fixture/repo/$(dirname "$path")"
    printf 'module fixture {}\n' > "$fixture/repo/$path"
done
cat > "$fixture/bin/quint" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$FIXTURE_LOG"
[[ "$*" != *"${FAIL_MATCH:-impossible-match}"* ]] || exit 37
if [[ "${MALFORMED_TRACE:-0}" == 1 ]]; then
    for arg in "$@"; do
        case "$arg" in --out-itf=*) printf 'malformed json\n' > "${arg#*=}";; esac
    done
    exit 0
fi
for arg in "$@"; do
    case "$arg" in --out-itf=*)
        if [[ "${OBSERVATION_TRACE:-0}" == 1 ]]; then
            printf '{"#meta":{},"states":[{"phase":{"#bigint":"0"}},{"phase":{"#bigint":"1"}},{"phase":{"#bigint":"2"}},{"phase":{"#bigint":"3"}},{"phase":{"#bigint":"4"}},{"phase":{"#bigint":"5"}},{"phase":{"#bigint":"6"}},{"phase":{"#bigint":"7"}},{"phase":{"#bigint":"8"}}]}\n' > "${arg#*=}"
        else
            printf '{"#meta":{},"states":[]}\n' > "${arg#*=}"
        fi;;
    esac
done
MOCK
cat > "$fixture/bin/nix" <<'MOCK'
#!/usr/bin/env bash
shift 2
case "$1" in quint) shift; exec quint "$@";; *) exit 0;; esac
MOCK
cat > "$fixture/bin/mktemp" <<'MOCK'
#!/usr/bin/env bash
path=$(/usr/bin/mktemp "$@") || exit
printf '%s\n' "$path" >> "$FIXTURE_TEMPS"
printf '%s\n' "$path"
MOCK
chmod +x "$fixture/bin/"*
export PATH="$fixture/bin:$PATH" TMPDIR="$fixture/tmp" FIXTURE_LOG="$fixture/commands" FIXTURE_TEMPS="$fixture/temps"
run() { local expected=$1; shift; local actual=0; "$@" > "$fixture/output" 2>&1 || actual=$?; [[ "$actual" == "$expected" ]]; }
cleaned() { while IFS= read -r path; do [[ ! -e "$path" ]]; done < "$FIXTURE_TEMPS"; }
: > "$FIXTURE_LOG"; : > "$FIXTURE_TEMPS"
for status in 0 37; do
    export FAIL_MATCH=impossible-match
    [[ "$status" == 0 ]] || export FAIL_MATCH='parse '
    run "$status" bash "$fixture/repo/scripts/verify/quint-workflow.sh" setup
    cleaned
 done
export FAIL_MATCH=impossible-match
: > "$FIXTURE_LOG"
run 0 bash "$fixture/repo/scripts/verify/quint-workflow.sh" check
[[ $(wc -l < "$FIXTURE_LOG") -eq 6 ]]
grep -q 'amp/nested.qnt' "$FIXTURE_LOG"
grep -q 'quint_specs/simulation.qnt' "$FIXTURE_LOG"
export FAIL_MATCH=amp/nested.qnt
: > "$FIXTURE_LOG"
run 1 bash "$fixture/repo/scripts/verify/quint-workflow.sh" check
[[ $(wc -l < "$FIXTURE_LOG") -eq 6 ]]
export FAIL_MATCH=impossible-match
: > "$FIXTURE_LOG"
run 0 bash "$fixture/repo/scripts/verify/quint-workflow.sh" models
[[ $(wc -l < "$FIXTURE_LOG") -eq 4 ]]
[[ $(grep -c '^verify --invariant=' "$FIXTURE_LOG") -eq 3 ]]
! grep -q 'amp/nested' "$FIXTURE_LOG"
printf '{"#meta":{},"states":[]}\n' > "$fixture/expected"
run 0 bash "$fixture/repo/scripts/verify/quint-trace.sh" check ignored.qnt "$fixture/expected"
cleaned
printf 'original output\n' > "$fixture/preserved"
export FAIL_MATCH='run '
run 37 bash "$fixture/repo/scripts/verify/quint-trace.sh" generate ignored.qnt "$fixture/preserved"
[[ $(cat "$fixture/preserved") == 'original output' ]]
cleaned
run 37 bash "$fixture/repo/scripts/verify/quint-trace.sh" check ignored.qnt "$fixture/expected"
cleaned
export MALFORMED_TRACE=1 FAIL_MATCH=impossible-match
run 5 bash "$fixture/repo/scripts/verify/quint-trace.sh" generate ignored.qnt "$fixture/preserved"
[[ $(cat "$fixture/preserved") == 'original output' ]]
cleaned
unset MALFORMED_TRACE
export TMPDIR="$fixture/quoted ' temporary"
mkdir -p "$TMPDIR"
export FAIL_MATCH=impossible-match
printf 'different\n' > "$fixture/expected"
run 1 bash "$fixture/repo/scripts/verify/quint-trace.sh" check ignored.qnt "$fixture/expected"
cleaned
# Observation conversion preserves both old outputs until generation and shape
# validation succeed. Each publication is atomic individually, not as a pair.
printf 'old trace\n' > "$fixture/observation.json"
printf 'old scenario\n' > "$fixture/scenario.toml"
for failure in generation malformed shape; do
    export FAIL_MATCH=impossible-match MALFORMED_TRACE=0 OBSERVATION_TRACE=0
    case "$failure" in generation) export FAIL_MATCH='run '; expected_exit=37;; malformed) export MALFORMED_TRACE=1; expected_exit=5;; shape) expected_exit=1;; esac
    run "$expected_exit" bash "$fixture/repo/scripts/verify/quint-observation.sh" ignored.qnt "$fixture/observation.json" "$fixture/scenario.toml"
    [[ $(cat "$fixture/observation.json") == 'old trace' ]]
    [[ $(cat "$fixture/scenario.toml") == 'old scenario' ]]
    cleaned
done
export FAIL_MATCH=impossible-match MALFORMED_TRACE=0 OBSERVATION_TRACE=1 QUINT_TRACE_SEED=991
: > "$FIXTURE_LOG"
run 0 bash "$fixture/repo/scripts/verify/quint-observation.sh" ignored.qnt "$fixture/observation.json" "$fixture/scenario.toml"
grep -q -- '--main=semantic_observation_smoke' "$FIXTURE_LOG"
grep -q -- '--seed=991' "$FIXTURE_LOG"
grep -q -- '--max-steps=16' "$FIXTURE_LOG"
jq -e '.states | length == 9' "$fixture/observation.json" >/dev/null
grep -q 'id = "quint-semantic-observation-smoke"' "$fixture/scenario.toml"
cleaned
run 2 bash "$fixture/repo/scripts/verify/quint-observation.sh" ignored.qnt "$fixture/observation.json" "$fixture/observation.json"
# Simulate the real boundary where the first rename succeeds and the second
# refuses: retain its status and accurately expose partial publication.
printf 'old trace\n' > "$fixture/observation.json"
printf 'old scenario\n' > "$fixture/scenario.toml"
real_mv=$(command -v mv)
export REAL_MV="$real_mv" REFUSED_SCENARIO="$fixture/scenario.toml"
cat > "$fixture/bin/mv" <<'MOCK'
#!/usr/bin/env bash
for arg in "$@"; do [[ "$arg" != "$REFUSED_SCENARIO" ]] || exit 73; done
exec "$REAL_MV" "$@"
MOCK
chmod +x "$fixture/bin/mv"
run 73 bash "$fixture/repo/scripts/verify/quint-observation.sh" ignored.qnt "$fixture/observation.json" "$fixture/scenario.toml"
jq -e '.states | length == 9' "$fixture/observation.json" >/dev/null
[[ $(cat "$fixture/scenario.toml") == 'old scenario' ]]
grep -q 'trace published.*scenario publication failed' "$fixture/output"
cleaned
rm "$fixture/bin/mv"
run 2 bash "$fixture/repo/scripts/verify/quint-observation.sh" ignored.qnt "$fixture/observation.json" "$fixture/./observation.json"
unset OBSERVATION_TRACE MALFORMED_TRACE QUINT_TRACE_SEED
# Source inventory cannot assert verification merely from a suggestive filename.
mkdir -p "$fixture/repo/verification/lean" "$fixture/repo/verification/quint/traces" "$fixture/repo/crates/aura-testkit/tests" "$fixture/repo/.github/workflows"
cp "$fixture/repo/verification/quint/root.qnt" "$fixture/repo/verification/quint/protocol_consensus.qnt"
printf '' > "$fixture/repo/justfile"
run 0 bash "$fixture/repo/scripts/verify/workflow.sh" coverage --json
jq -e '.schema_version == 2 and .evidence.status == "unknown" and .evidence.receipts_consumed == false and (.metrics | has("verified_specs") | not)' "$fixture/output" > /dev/null
run 0 bash "$fixture/repo/scripts/verify/workflow.sh" coverage --md
grep -q 'unknown (no receipts consumed)' "$fixture/output"
! grep -q 'CI verified' "$fixture/output"
printf 'Quint wrapper contracts PASS (fake commands; no model proof)\n'
