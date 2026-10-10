#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-kani-inventory.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/bin" "$fixture/repo/scripts/verify"
cp "$here/kani-consensus.sh" "$here/kani-consensus-inventory.txt" "$fixture/repo/scripts/verify/"
caller_artifacts="$fixture/caller-artifacts"
mkdir -p "$caller_artifacts"
printf 'caller-owned\n' > "$caller_artifacts/sentinel"
# Exercise a foreign inherited path, then pin all runner artifacts locally.
export AURA_KANI_ARTIFACTS="$caller_artifacts"
export AURA_KANI_ARTIFACTS="$fixture/repo/artifacts/kani"
export KANI_FIXTURE_EXPECTED="$here/kani-consensus-inventory.txt"
# Independently enforce the complete Cartesian partition domain.
for phase in 0 1 2 3 4; do
  for transition in 0 1 2; do
    for binding in 0 1 2 3 4 5 6 7; do
      printf 'core::verification::kani_proofs::production_wrappers_refine_bounded_decisions_p%s_t%s_b%s\n' "$phase" "$transition" "$binding"
    done
  done
done | sort > "$fixture/family"
rg '::production_wrappers_' "$KANI_FIXTURE_EXPECTED" > "$fixture/registered-family"
diff -u "$fixture/family" "$fixture/registered-family"
cat > "$fixture/bin/just" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == *' list '* ]]; then
  [[ "$KANI_FIXTURE_CASE" != discovery-error ]] || exit 29
[[ "$KANI_FIXTURE_CASE" != no-json ]] || exit 0
version=0.68.0; schema=0.1
[[ "$KANI_FIXTURE_CASE" != foreign-version ]] || version=0.69.0
[[ "$KANI_FIXTURE_CASE" != foreign-schema ]] || schema=0.2
names=$(cat "$KANI_FIXTURE_EXPECTED")
[[ "$KANI_FIXTURE_CASE" != empty ]] || names=''
[[ "$KANI_FIXTURE_CASE" != missing ]] || names=$(tail -n +2 "$KANI_FIXTURE_EXPECTED")
[[ "$KANI_FIXTURE_CASE" != extra ]] || names+=$'\ncore::verification::kani_proofs::foreign_proof'
[[ "$KANI_FIXTURE_CASE" != duplicate ]] || names+=$'\n'$(head -n1 "$KANI_FIXTURE_EXPECTED")
jq -n --arg version "$version" --arg schema "$schema" --arg names "$names" '{"kani-version":$version,"file-version":$schema,"standard-harnesses":{"source.rs":($names|split("\n")|map(select(length>0)))},"contract-harnesses":{},contracts:[],totals:{"standard-harnesses":134,"contract-harnesses":0,"functions-under-contract":0}}' > kani-list.json
exit 0
else
  [[ "$KANI_FIXTURE_CASE" != proof-error ]] || { echo actual-proof-failure; exit 42; }
  while IFS= read -r harness; do
    [[ "$KANI_FIXTURE_CASE" != partial || "$harness" != *p1_t0_b0 ]] || continue
    [[ "$KANI_FIXTURE_CASE" != threaded ]] || printf 'Thread 0: '
    printf 'Checking harness %s...\n' "$harness"
  done < "$KANI_FIXTURE_EXPECTED"
fi
MOCK
chmod +x "$fixture/bin/just"
export PATH="$fixture/bin:$PATH"
for KANI_FIXTURE_CASE in success empty missing extra partial discovery-error proof-error foreign-version foreign-schema no-json duplicate threaded stale; do
  export KANI_FIXTURE_CASE
  [[ "$KANI_FIXTURE_CASE" != stale ]] || echo stale > "$fixture/repo/kani-list.json"
  status=0
  (cd "$fixture/repo" && bash scripts/verify/kani-consensus.sh) > "$fixture/$KANI_FIXTURE_CASE.log" 2>&1 || status=$?
  case "$KANI_FIXTURE_CASE" in
    success|threaded) [[ "$status" == 0 ]] ;;
duplicate)
  [[ "$status" != 0 ]]
  [[ $(wc -l < "$fixture/repo/artifacts/kani/discovered.txt" | tr -d ' ') == 135 ]]
  [[ $(sort -u "$fixture/repo/artifacts/kani/discovered.txt" | wc -l | tr -d ' ') == 134 ]]
  ;;
discovery-error) [[ "$status" == 29 ]] ;;
    proof-error) [[ "$status" == 42 ]]; rg -F actual-proof-failure "$fixture/repo/artifacts/kani/proofs.log" ;;
    *) [[ "$status" != 0 ]] ;;
  esac
  printf 'canonical Kani inventory fixture %s PASS\n' "$KANI_FIXTURE_CASE"
done

[[ $(cat "${caller_artifacts}/sentinel") == caller-owned ]]
[[ $(find "${caller_artifacts}" -type f | wc -l | tr -d ' ') == 1 ]]
printf 'canonical Kani caller artifact isolation PASS\n'
