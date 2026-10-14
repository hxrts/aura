#!/usr/bin/env bash
# One source-frozen compiler invocation, then exact discovery and execution
# against the produced libtest binary. The full ownership gate remains separate.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
inside=0
if [[ "${1:-}" == --inside ]]; then inside=1; shift; fi
original=("$@")
root="$repo_root" lane=ownership-exact package='' target='' binary='' features='' count=''
names=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --root) root="${2:?missing root}"; shift 2 ;;
    --lane) lane="${2:?missing lane}"; shift 2 ;;
    --package) package="${2:?missing package}"; shift 2 ;;
    --lib) [[ -z "$target" ]] || exit 2; target=lib; shift ;;
    --test) [[ -z "$target" ]] || exit 2; target=test; binary="${2:?missing binary}"; shift 2 ;;
    --features) features="${2:?missing features}"; shift 2 ;;
    --expected-count) count="${2:?missing count}"; shift 2 ;;
    --name) names+=("${2:?missing test name}"); shift 2 ;;
    *) echo 'run-exact-fixtures: unknown option' >&2; exit 2 ;;
  esac
done
[[ "$package" =~ ^[A-Za-z0-9_-]+$ && "$lane" =~ ^[A-Za-z0-9_-]+$ &&
   ( "$target" == lib || "$target" == test ) && "$count" =~ ^[1-9][0-9]*$ ]] || {
  echo 'run-exact-fixtures: package, target and positive expected count required' >&2; exit 2;
}
[[ "$target" != test || "$binary" =~ ^[A-Za-z0-9_-]+$ ]] || exit 2
[[ -z "$features" || "$features" =~ ^[A-Za-z0-9_-]+(,[A-Za-z0-9_-]+)*$ ]] || exit 2
[[ "${#names[@]}" -eq "$count" ]] || { echo 'run-exact-fixtures: inventory count mismatch' >&2; exit 2; }
for name in "${names[@]}"; do
  [[ "$name" =~ ^[A-Za-z0-9_:]+$ ]] || { echo 'run-exact-fixtures: invalid exact name' >&2; exit 2; }
done
[[ "$(printf '%s\n' "${names[@]}" | sort -u | wc -l | tr -d ' ')" == "$count" ]] || {
  echo 'run-exact-fixtures: duplicate exact name' >&2; exit 2;
}
root="$(cd "$root" && pwd -P)"
[[ -f "$root/Cargo.toml" ]] || exit 2
inventory="$(printf '%s\n' "${names[@]}")"
if (( inside == 0 )); then
  export AURA_BUILD_PACKAGES="$package" AURA_BUILD_FEATURES="$features"
  export AURA_BUILD_TEST_INVENTORY="$inventory"
  exec bash "$repo_root/scripts/dev/build-budget.sh" --root "$root" --lane "$lane" -- \
    bash "$repo_root/scripts/dev/run-exact-fixtures.sh" --inside "${original[@]}"
fi
[[ -n "${AURA_BUILD_RECEIPT:-}" && -f "$AURA_BUILD_RECEIPT" ]] || {
  echo 'run-exact-fixtures: original budget receipt required' >&2; exit 2;
}
source "$repo_root/scripts/dev/build-receipt.sh"
frozen="$(jq -r '.source.fingerprint // empty' "$AURA_BUILD_RECEIPT")"
[[ -n "$frozen" && "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
  echo 'run-exact-fixtures: source changed before compile' >&2; exit 1;
}
inventory_file="$(mktemp "$(dirname "$AURA_BUILD_RECEIPT")/inventory.XXXXXX")"
chmod 600 "$inventory_file"
printf '%s\n' "${names[@]}" > "$inventory_file"
aura_receipt_update --arg file "$inventory_file" --argjson count "$count" \
  '.artifacts.expected_inventory=$file | .metadata.expected_count=$count'
cargo_args=(test -p "$package")
if [[ "$target" == lib ]]; then cargo_args+=(--lib); else cargo_args+=(--test "$binary"); fi
if [[ -n "$features" ]]; then cargo_args+=(--features "$features"); fi
bash "$repo_root/scripts/dev/build-phase.sh" compile -- cargo "${cargo_args[@]}" --no-run --message-format=json
compile_log="$(jq -r '[.phases[] | select(.phase=="compile") | .log][-1]' "$AURA_BUILD_RECEIPT")"
executables=()
while IFS= read -r executable; do [[ -z "$executable" ]] || executables+=("$executable"); done < <(
  jq -Rr 'fromjson? | select(.reason=="compiler-artifact" and .executable!=null and .target.test==true) | .executable' "$compile_log"
)
[[ "${#executables[@]}" -eq 1 && -f "${executables[0]:-}" && -x "${executables[0]:-}" ]] || {
  echo 'run-exact-fixtures: compile did not produce exactly one test executable' >&2; exit 1;
}
executable="${executables[0]}"
[[ "$executable" == "$root/target/"* && ! -L "$executable" ]] || {
  echo 'run-exact-fixtures: test executable outside selected target' >&2; exit 1;
}
aura_receipt_update --arg executable "$executable" '.artifacts.test_executable=$executable'
for name in "${names[@]}"; do
  [[ "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
    echo 'run-exact-fixtures: source changed during batch' >&2; exit 1;
  }
  expected="$(mktemp "$(dirname "$AURA_BUILD_RECEIPT")/expected.XXXXXX")"
  chmod 600 "$expected"
  printf '%s\n' "$name" > "$expected"
  bash "$repo_root/scripts/dev/build-phase.sh" discovery --expected-tests "$expected" -- \
    "$executable" "$name" --exact --list
  bash "$repo_root/scripts/dev/build-phase.sh" execution --expected-tests "$expected" -- \
    "$executable" "$name" --exact --format pretty --nocapture
done
[[ "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
  echo 'run-exact-fixtures: source changed during batch' >&2; exit 1;
}
