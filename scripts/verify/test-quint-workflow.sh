#!/usr/bin/env bash
# Contract fixtures use fake commands; they establish wrapper behavior, not proofs.
set -euo pipefail
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-quint-contract.XXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
mkdir -p "$fixture/repo/scripts/verify" "$fixture/bin" "$fixture/tmp"
cp "$script_dir/"{quint-workflow,quint-trace,workflow}.sh "$fixture/repo/scripts/verify/"
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
    case "$arg" in --out-itf=*) printf '{"#meta":{},"states":[]}\n' > "${arg#*=}";; esac
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
