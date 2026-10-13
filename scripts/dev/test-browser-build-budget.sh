#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture=$(mktemp -d /tmp/aura-browser-admission.XXXXXX)
fixture=$(cd "$fixture" && pwd -P)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/scripts/ci" "$fixture/scripts/dev" "$fixture/crates/aura-harness/playwright-driver" "$fixture/fakebin" "$fixture/target/aura-web-tools-ci"
cp "$repo_root/scripts/ci/browser-smoke.sh" "$repo_root/scripts/ci/web-matrix.sh" "$fixture/scripts/ci/"
printf 'retained\n' > "$fixture/target/aura-web-tools-ci/sentinel"
cat > "$fixture/scripts/dev/build-budget.sh" <<'BUDGET'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\0' "$@" > "$FIXTURE_ROOT/admission-argv"
[[ ${AURA_TEST_BROWSER_BUDGET_DEPTH:-0} == 0 ]] || exit 97
export AURA_TEST_BROWSER_BUDGET_DEPTH=1
[[ ${ADMISSION_RESULT:-0} == 0 ]] || exit "$ADMISSION_RESULT"
[[ "$1" == --lane && "$2" == browser-ci && "$3" == -- ]]
shift 3
export AURA_BUILD_BUDGET_HELD="$FIXTURE_ROOT"
exec "$@"
BUDGET
cat > "$fixture/fakebin/npm" <<'NPM'
#!/usr/bin/env bash
[[ $AURA_BUILD_BUDGET_HELD == "$FIXTURE_ROOT" ]] || exit 99
printf 'owned\n' > "$FIXTURE_ROOT/npm-owned"
exit "${NPM_RESULT:-37}"
NPM
chmod +x "$fixture/fakebin/npm"
export FIXTURE_ROOT="$fixture"
export PATH="$fixture/fakebin:$PATH"
unset AURA_BUILD_BUDGET_HELD
export ADMISSION_RESULT=0
export AURA_TEST_BROWSER_BUDGET_DEPTH=0
export NPM_RESULT=37
ln -s "$fixture" "$fixture/checkout-alias"
for script in browser-smoke web-matrix; do
  rm -rf "$fixture/artifacts"
  rm -f "$fixture/npm-owned"
  status=0
  ADMISSION_RESULT=75 bash "$fixture/scripts/ci/$script.sh" 'argument with spaces' > "$fixture/refusal.log" 2>&1 || status=$?
  if [[ $status != 75 || -e "$fixture/artifacts" || -e "$fixture/npm-owned" ]]; then
    echo "$script bypassed admission: status=$status, expected refusal75 before mutation" >&2
    exit 1
  fi
  [[ $(cat "$fixture/target/aura-web-tools-ci/sentinel") == retained ]]
  status=0
  bash "$fixture/scripts/ci/$script.sh" 'argument with spaces' > "$fixture/admitted.log" 2>&1 || status=$?
  [[ $status == 37 && $(cat "$fixture/npm-owned") == owned ]]
  [[ $(cat "$fixture/target/aura-web-tools-ci/sentinel") == retained ]]
  argv=()
  while IFS= read -r -d '' arg; do argv+=("$arg"); done < "$fixture/admission-argv"
  [[ ${#argv[@]} == 6 && ${argv[5]} == 'argument with spaces' ]]
  status=0
  bash "$fixture/checkout-alias/scripts/ci/$script.sh" 'argument with spaces' > "$fixture/alias.log" 2>&1 || status=$?
  [[ $status == 37 && $(cat "$fixture/npm-owned") == owned ]]
  echo "$script admission/refusal/argv/exit/cache fixtures passed"
done

# Exercise asset preparation and final harness commands without a compiler or
# browser. Every tool must inherit the original admitted checkout owner.
mkdir -p "$fixture/scripts/web" "$fixture/scripts/harness" "$fixture/crates/aura-web/public/assets"
: > "$fixture/crates/aura-web/public/assets/tailwind.css"
for command in cargo dx; do
  cat > "$fixture/fakebin/$command" <<'TOOL'
#!/usr/bin/env bash
[[ $AURA_BUILD_BUDGET_HELD == "$FIXTURE_ROOT" ]] || exit 99
printf '%s\n' "${0##*/}" >> "$FIXTURE_ROOT/owned-tools"
TOOL
  chmod +x "$fixture/fakebin/$command"
done
cp "$fixture/fakebin/dx" "$fixture/scripts/web/dx.sh"
cat > "$fixture/scripts/harness/run-matrix.sh" <<'MATRIX'
#!/usr/bin/env bash
[[ $AURA_BUILD_BUDGET_HELD == "$FIXTURE_ROOT" ]] || exit 99
[[ $# == 3 && $1 == --lane && $2 == web && $3 == 'argument with spaces' ]] || exit 98
exit 53
MATRIX
export NPM_RESULT=0
for script in browser-smoke web-matrix; do
  status=0
  bash "$fixture/scripts/ci/$script.sh" 'argument with spaces' > "$fixture/full.log" 2>&1 || status=$?
  expected=0
  [[ $script != web-matrix ]] || expected=53
  if [[ $status != "$expected" ]]; then
    cat "$fixture/full.log" "$fixture/artifacts/harness/browser/"*.log >&2
    exit 1
  fi
  [[ $(cat "$fixture/target/aura-web-tools-ci/sentinel") == retained ]]
  echo "$script complete owned tool/harness/cache fixtures passed"
done

# Preserve actual compiler diagnostics and final status across the existing
# two-attempt asset-directory recovery; never run the scenario after failure.
cat > "$fixture/scripts/web/dx.sh" <<'DX'
#!/usr/bin/env bash
set -euo pipefail
[[ $AURA_BUILD_BUDGET_HELD == "$FIXTURE_ROOT" ]] || exit 99
attempt=0
[[ ! -f "$FIXTURE_ROOT/dx-attempt" ]] || attempt=$(cat "$FIXTURE_ROOT/dx-attempt")
attempt=$((attempt + 1))
printf '%s\n' "$attempt" > "$FIXTURE_ROOT/dx-attempt"
printf 'compiler stdout attempt%s\n' "$attempt"
printf 'compiler stderr attempt%s\n' "$attempt" >&2
[[ "$attempt" != 1 ]] || exit 17
exit "$DX_SECOND_RESULT"
DX
for second_result in 23 0; do
  rm -f "$fixture/dx-attempt" "$fixture/owned-tools"
  status=0
  DX_SECOND_RESULT="$second_result" bash "$fixture/scripts/ci/browser-smoke.sh" > "$fixture/dx-case.log" 2>&1 || status=$?
  [[ "$status" == "$second_result" && $(cat "$fixture/dx-attempt") == 2 ]]
  for attempt in 1 2; do
    for stream in stdout stderr; do
      rg -Fxq "compiler $stream attempt$attempt" "$fixture/artifacts/harness/browser/ci-browser.log"
    done
  done
  expected_cargo_calls=1
  [[ "$second_result" != 0 ]] || expected_cargo_calls=2
  [[ $(rg -c '^cargo$' "$fixture/owned-tools") == "$expected_cargo_calls" ]]
  echo "browser-smoke retained dx diagnostics/final status $second_result fixtures passed"
done
