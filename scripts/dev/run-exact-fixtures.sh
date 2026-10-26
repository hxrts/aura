#!/usr/bin/env bash
# One source-frozen compiler invocation, then exact discovery and execution
# against the produced libtest binary. The full ownership gate remains separate.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
inside=0
if [[ "${1:-}" == --inside ]]; then inside=1; shift; fi
original=("$@")
root="$repo_root" lane=ownership-exact package='' target='' binary='' features='' count='' timings=0 decisive='' replay_first=0
names=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --root) root="${2:?missing root}"; shift 2 ;;
    --lane) lane="${2:?missing lane}"; shift 2 ;;
    --package) package="${2:?missing package}"; shift 2 ;;
    --lib) [[ -z "$target" ]] || exit 2; target=lib; shift ;;
    --test) [[ -z "$target" ]] || exit 2; target=test; binary="${2:?missing binary}"; shift 2 ;;
    --features) features="${2:?missing features}"; shift 2 ;;
    --timings) timings=1; shift ;;
    --replay-first-failure) replay_first=1; shift ;;
    --decisive-name) [[ -z "$decisive" ]] || exit 2; decisive="${2:?missing decisive name}"; shift 2 ;;
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
# An explicit checkpoint is the first declared member of the complete inventory.
# Later discovery remains mandatory for batch success; default ordering is intact.
[[ -z "$decisive" || "$decisive" == "${names[0]}" ]] || {
  echo 'run-exact-fixtures: decisive name must be the first declared exact name' >&2; exit 2;
}
if (( replay_first == 1 )); then
  command -v b3sum >/dev/null || { echo 'run-exact-fixtures: replay requires b3sum' >&2; exit 2; }
fi
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
aura_receipt_update --arg file "$inventory_file" --argjson count "$count" --arg decisive "$decisive" \
  '.artifacts.expected_inventory=$file | .metadata.expected_count=$count | .metadata.decisive_name=(if $decisive=="" then null else $decisive end)'
cargo_args=(test -p "$package")
if [[ "$target" == lib ]]; then cargo_args+=(--lib); else cargo_args+=(--test "$binary"); fi
if [[ -n "$features" ]]; then cargo_args+=(--features "$features"); fi
if (( timings == 1 )); then cargo_args+=(--timings); fi
bash "$repo_root/scripts/dev/build-phase.sh" compile -- cargo "${cargo_args[@]}" --no-run --message-format=json
compile_log="$(jq -r '[.phases[] | select(.phase=="compile") | .log][-1]' "$AURA_BUILD_RECEIPT")"
executables=()
while IFS= read -r executable; do [[ -z "$executable" ]] || executables+=("$executable"); done < <(
  jq -Rr --arg target "$binary" --arg kind "$target" '
    fromjson? | select(.reason=="compiler-artifact" and .executable!=null and
      ((($kind=="test") and .target.name==$target and (.target.kind | index("test")!=null)) or
       (($kind=="lib") and (.target.kind | index("lib")!=null) and .target.test==true))) |
    .executable' "$compile_log"
)
[[ "${#executables[@]}" -eq 1 && -f "${executables[0]:-}" && -x "${executables[0]:-}" ]] || {
  echo 'run-exact-fixtures: compile did not produce exactly one test executable' >&2; exit 1;
}
executable="${executables[0]}"
[[ "$executable" == "$root/target/"* && ! -L "$executable" ]] || {
  echo 'run-exact-fixtures: test executable outside selected target' >&2; exit 1;
}
aura_receipt_update --arg executable "$executable" '.artifacts.test_executable=$executable'
expected_files=()
expected_digests=()
# Opt-in replay keeps its original expectations in this process, never reads an
# expected digest back from a child-writable receipt. Existing phases own output.
replay_configuration() {
  local variable
  for variable in CARGO_BUILD_TARGET CARGO_TARGET_DIR CARGO_INCREMENTAL RUSTC RUSTC_WRAPPER \
    RUSTC_WORKSPACE_WRAPPER RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_HOME RUSTUP_TOOLCHAIN \
    CARGO_PROFILE_DEV_DEBUG CARGO_PROFILE_TEST_DEBUG CARGO_BUILD_JOBS PATH \
    CARGO_PROFILE_DEV_INCREMENTAL CARGO_PROFILE_TEST_INCREMENTAL RUSTUP_HOME \
    AURA_NIX_CARGO_BIN AURA_NIX_CLIPPY_BIN; do
    if [[ ${!variable+x} ]]; then printf '%s\0set\0%s\0' "$variable" "${!variable}";
    else printf '%s\0unset\0' "$variable"; fi
  done
}
replay_receipt_configuration() {
  jq -ce '{root,source,command_argv,metadata:(.metadata|{packages,features,test_target,
    test_filter,test_inventory,expected_count,decisive_name,producer_binary_blake3}),
    artifacts:(.artifacts|{expected_inventory,test_executable,protected_test_executable,
      protected_test_executable_blake3})}' "$AURA_BUILD_RECEIPT"
}
replay_helpers=(run-exact-fixtures.sh build-budget.sh build-phase.sh build-receipt.sh
  libtest-inventory.awk scoped-builders.sh cache-rank.sh disk-report.sh policy-dependency-inputs.sh)
replay_helper_digests=()
replay_binary_digest='' replay_protected='' replay_protected_digest=''
replay_inventory_digest='' replay_config='' replay_receipt_config=''
if (( replay_first == 1 )); then
  replay_binary_digest=$(b3sum "$executable") || exit 78
  protected_directory=$(mktemp -d "$(dirname "$AURA_BUILD_RECEIPT")/test-binary.XXXXXX") || exit 78
  chmod 700 "$protected_directory"
  replay_protected="$protected_directory/$(basename "$executable")"
  cp -p "$executable" "$replay_protected" || exit 78
  chmod 500 "$replay_protected" || exit 78
  replay_protected_digest=$(b3sum "$replay_protected") || exit 78
  [[ ${replay_protected_digest%% *} == "${replay_binary_digest%% *}" ]] || exit 78
  observed=$(b3sum "$executable") || exit 78
  [[ $observed == "$replay_binary_digest" ]] || exit 78
  replay_inventory_digest=$(b3sum "$inventory_file") || exit 78
  replay_config=$(replay_configuration | b3sum) || exit 78
  for helper in "${replay_helpers[@]}"; do
    [[ -f "$repo_root/scripts/dev/$helper" && ! -L "$repo_root/scripts/dev/$helper" ]] || exit 78
    digest=$(b3sum "$repo_root/scripts/dev/$helper") || exit 78
    replay_helper_digests+=("$digest")
  done
  aura_receipt_update --arg digest "$replay_binary_digest" --arg copy "$replay_protected" \
    --arg copy_digest "$replay_protected_digest" \
    '.metadata.replay_first_failure=true | .metadata.producer_binary_blake3=$digest |
      .artifacts.protected_test_executable=$copy | .artifacts.protected_test_executable_blake3=$copy_digest'
  replay_receipt_config=$(replay_receipt_configuration) || exit 78
fi
replay_guard() {
  local observed index
  [[ -f "$executable" && -x "$executable" && ! -L "$executable" ]] || return 78
  observed=$(b3sum "$executable") || return 78
  [[ $observed == "$replay_binary_digest" ]] || return 78
  [[ -f "$replay_protected" && -x "$replay_protected" && ! -L "$replay_protected" ]] || return 78
  observed=$(b3sum "$replay_protected") || return 78
  [[ $observed == "$replay_protected_digest" ]] || return 78
  observed=$(aura_receipt_source_fingerprint "$root") || return 78
  [[ $observed == "$frozen" ]] || return 78
  observed=$(replay_configuration | b3sum) || return 78
  [[ $observed == "$replay_config" ]] || return 78
  observed=$(replay_receipt_configuration) || return 78
  [[ $observed == "$replay_receipt_config" ]] || return 78
  [[ -f "$inventory_file" && ! -L "$inventory_file" ]] || return 78
  observed=$(b3sum "$inventory_file") || return 78
  [[ $observed == "$replay_inventory_digest" ]] || return 78
  for index in "${!replay_helpers[@]}"; do
    [[ -f "$repo_root/scripts/dev/${replay_helpers[$index]}" && ! -L "$repo_root/scripts/dev/${replay_helpers[$index]}" ]] || return 78
    observed=$(b3sum "$repo_root/scripts/dev/${replay_helpers[$index]}") || return 78
    [[ $observed == "${replay_helper_digests[$index]}" ]] || return 78
  done
  for index in "${!expected_files[@]}"; do
    [[ -f "${expected_files[$index]}" && ! -L "${expected_files[$index]}" ]] || return 78
    observed=$(b3sum "${expected_files[$index]}") || return 78
    [[ $observed == "${expected_digests[$index]}" ]] || return 78
  done
}
execute_exact() {
  local name=$1 expected=$2 original_status=0 replay_status=0 guard_status=0 post_guard_status=0
  if (( replay_first == 1 )); then replay_guard || return 78; fi
  bash "$repo_root/scripts/dev/build-phase.sh" execution --expected-tests "$expected" -- \
    "$executable" "$name" --exact --format pretty --nocapture || original_status=$?
  if (( original_status == 0 )); then return 0; fi
  if (( replay_first == 1 )); then
    replay_guard || guard_status=$?
    if (( guard_status == 0 )); then
      bash "$repo_root/scripts/dev/build-phase.sh" execution --expected-tests "$expected" -- \
        "$executable" "$name" --exact --format pretty --nocapture || replay_status=$?
      replay_guard || post_guard_status=$?
    else
      echo 'run-exact-fixtures: unchanged original identities unavailable; replay refused' >&2
    fi
    aura_receipt_update --arg name "$name" --argjson original "$original_status" \
      --argjson replay "$replay_status" --argjson refusal "$guard_status" --argjson post "$post_guard_status" \
      '.metadata.first_failure_replay={test:$name,original_exit_status:$original,
        replay_exit_status:(if $refusal==0 then $replay else null end),
        refusal_exit_status:(if $refusal==0 then null else $refusal end),
        post_replay_refusal_exit_status:(if $post==0 then null else $post end)}' || {
          echo 'run-exact-fixtures: replay metadata retention failed; original failure retained' >&2 || :;
        }
  fi
  return "$original_status"
}
for name in "${names[@]}"; do
  [[ "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
    echo 'run-exact-fixtures: source changed during batch' >&2; exit 1;
  }
  expected="$(mktemp "$(dirname "$AURA_BUILD_RECEIPT")/expected.XXXXXX")"
  chmod 600 "$expected"
  printf '%s\n' "$name" > "$expected"
  expected_files+=("$expected")
  if (( replay_first == 1 )); then
    digest=$(b3sum "$expected") || exit 78
    expected_digests+=("$digest")
  fi
  bash "$repo_root/scripts/dev/build-phase.sh" discovery --expected-tests "$expected" -- \
    "$executable" "$name" --exact --list
  if [[ -n "$decisive" && "$name" == "$decisive" ]]; then
    [[ "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
      echo 'run-exact-fixtures: source changed before decisive execution' >&2; exit 1;
    }
    execute_exact "$name" "$expected"
  fi
done
# Default mode discovers everything first. Decisive mode has executed only its
# checkpoint; every adjacent declaration still must match before adjacent execution.
for index in "${!names[@]}"; do
  [[ "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
    echo 'run-exact-fixtures: source changed during batch' >&2; exit 1;
  }
  name="${names[$index]}"
  if [[ -n "$decisive" && "$name" == "$decisive" ]]; then continue; fi
  expected="${expected_files[$index]}"
  execute_exact "$name" "$expected"
done
if (( replay_first == 1 )); then replay_guard || exit 78; fi
[[ "$frozen" == "$(aura_receipt_source_fingerprint "$root")" ]] || {
  echo 'run-exact-fixtures: source changed during batch' >&2; exit 1;
}
