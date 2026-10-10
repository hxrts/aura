#!/usr/bin/env bash
set -euo pipefail
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-kani-runner.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/repo/scripts/verify" "$fixture/bin"
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cp "$script_dir/workflow.sh" "$fixture/repo/scripts/verify/workflow.sh"
cat > "$fixture/bin/just" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" > "$KANI_FIXTURE_ARGS"
printf 'actual canonical gate output\n'
exit "$KANI_FIXTURE_EXIT"
MOCK
chmod +x "$fixture/bin/just"
export PATH="$fixture/bin:$PATH" KANI_FIXTURE_ARGS="$fixture/args"
for expected in 0 42; do
    rm -f "$fixture/repo"/logs/kani/*.log
    export KANI_FIXTURE_EXIT="$expected"
    actual=0
    bash "$fixture/repo/scripts/verify/workflow.sh" kani > "$fixture/output" 2>&1 || actual=$?
    [[ "$actual" == "$expected" ]]
    [[ "$(cat "$fixture/args")" == 'ci-kani' ]]
    [[ "$(cat "$fixture/repo"/logs/kani/*.log)" == 'actual canonical gate output' ]]
    printf 'canonical runner status/log fixture %s PASS\n' "$expected"
done
cat > "$fixture/bin/tee" <<'MOCK'
#!/usr/bin/env bash
cat > "$1"
exit 73
MOCK
chmod +x "$fixture/bin/tee"
for expected_gate in 0 42; do
    rm -f "$fixture/repo"/logs/kani/*.log
    export KANI_FIXTURE_EXIT="$expected_gate"
    actual=0
    bash "$fixture/repo/scripts/verify/workflow.sh" kani > "$fixture/output" 2>&1 || actual=$?
    expected=73
    if [[ "$expected_gate" == 42 ]]; then expected=42; fi
    [[ "$actual" == "$expected" ]]
    [[ "$(cat "$fixture/args")" == 'ci-kani' ]]
    [[ "$(cat "$fixture/repo"/logs/kani/*.log)" == 'actual canonical gate output' ]]
    printf 'canonical runner gate %s/logger 73 returns %s and retains log PASS\n' "$expected_gate" "$expected"
done
