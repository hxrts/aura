#!/usr/bin/env bash
# Reject ignored additions and check canonical owners of staged Rust changes.
set -euo pipefail
root=$(git rev-parse --show-toplevel)
root=$(cd "$root" && pwd -P)
cd "$root"
unset CARGO_TARGET_DIR
export CARGO_BUILD_JOBS=4
export AURA_BUILD_WAIT_SECONDS=${AURA_BUILD_WAIT_SECONDS:-1800}
manifests=()
packages=()
count=0
head_paths=()
head_manifests=()
head_loaded=false
head_root=''
removed_owners=()
staged_list=$(mktemp)
index_list=$(mktemp)
cleanup() {
  rm -f "$staged_list" "$index_list"
  [[ -z "$head_root" ]] || rm -rf "$head_root"
}
trap cleanup EXIT
# One NUL-delimited status/path owner distinguishes deletion from addition.
# Rename source and destination remain separate canonical owners.
git diff --no-relative --no-renames --cached --name-status --diff-filter=ACMRD -z > "$staged_list"
git ls-files -z > "$index_list"
index_has() {
  local result
  if git --literal-pathspecs ls-files --error-unmatch -- "$1" >/dev/null 2>&1; then return 0; else result=$?; fi
  [[ "$result" == 1 ]] && return 1
  exit "$result"
}
package_for_manifest() {
  local manifest=$1 metadata
  metadata=$(cargo metadata --no-deps --locked --format-version 1 --manifest-path "$manifest") || return "$?"
  jq -er --arg manifest "$manifest" '
    [.packages[] | select(.manifest_path == $manifest)] |
    if length == 1 then .[0].name else error("staged Rust file has no unique owning Cargo package") end
  ' <<< "$metadata"
}
queue_manifest() {
  local relative=$1 manifest="$root/$1" index
  for ((index=0; index<count; index++)); do
    [[ "${manifests[index]}" != "$manifest" ]] || return 0
  done
  [[ -f "$manifest" ]] || { echo "indexed Cargo manifest absent from working tree: $relative" >&2; exit 1; }
  manifests[count]=$manifest
  packages[count]=$(package_for_manifest "$manifest")
  count=$((count + 1))
}
find_index_manifest() {
  local directory candidate
  directory=${1%/*}; [[ "$directory" != "$1" ]] || directory=.
  while :; do
    if [[ "$directory" == . ]]; then candidate=Cargo.toml; else candidate="$directory/Cargo.toml"; fi
    if index_has "$candidate"; then printf '%s\n' "$candidate"; return 0; fi
    [[ "$directory" != . ]] || return 1
    if [[ "$directory" == */* ]]; then directory=${directory%/*}; else directory=.; fi
  done
}
load_head() {
  local head_list path result
  [[ "$head_loaded" == false ]] || return 0
  head_list=$(mktemp)
  if git ls-tree --name-only -r -z HEAD > "$head_list"; then :; else result=$?; rm -f "$head_list"; return "$result"; fi
  while IFS= read -r -d '' path; do
    head_paths+=("$path")
    [[ "${path##*/}" != Cargo.toml ]] || head_manifests+=("$path")
  done < "$head_list"
  rm -f "$head_list"
  head_loaded=true
}
find_head_manifest() {
  local directory candidate manifest
  directory=${1%/*}; [[ "$directory" != "$1" ]] || directory=.
  while :; do
    if [[ "$directory" == . ]]; then candidate=Cargo.toml; else candidate="$directory/Cargo.toml"; fi
    for manifest in ${head_manifests[@]+"${head_manifests[@]}"}; do
      if [[ "$manifest" == "$candidate" ]]; then printf '%s\n' "$manifest"; return 0; fi
    done
    [[ "$directory" != . ]] || return 1
    if [[ "$directory" == */* ]]; then directory=${directory%/*}; else directory=.; fi
  done
}
prove_removed_owner() {
  local owner=$1 directory path nearest proved
  for proved in ${removed_owners[@]+"${removed_owners[@]}"}; do [[ "$proved" != "$owner" ]] || return 0; done
  if [[ -z "$head_root" ]]; then
    head_root=$(mktemp -d "${TMPDIR:-/tmp}/aura-staged-head.XXXXXX")
    # Cargo, not a TOML parser, proves the exact original package in its full
    # trusted workspace/dependency context. No working tree is opened or reset.
    git archive --format=tar HEAD | tar -xf - -C "$head_root"
  fi
  package_for_manifest "$head_root/$owner" >/dev/null
  directory=${owner%/*}; [[ "$directory" != "$owner" ]] || directory=.
  for path in ${head_paths[@]+"${head_paths[@]}"}; do
    [[ "$directory" == . || "$path" == "$directory/"* ]] || continue
    nearest=$(find_head_manifest "$path") || continue
    [[ "$nearest" == "$owner" ]] || continue
    if index_has "$path"; then
      echo "removed Cargo manifest still owns indexed file: $path" >&2
      exit 1
    fi
  done
  while IFS= read -r -d '' path; do
    [[ "$directory" == . || "$path" == "$directory/"* ]] || continue
    nearest=$(find_index_manifest "$path") || {
      owner_status=$?; [[ "$owner_status" == 1 ]] || exit "$owner_status"
      echo "removed package retains a file without a canonical indexed owner: $path" >&2; exit 1;
    }
    [[ "$nearest" != "$owner" && ( "$directory" == . || "$nearest" == "$directory/"* ) ]] || {
      echo "removed package retains a file outside a nested package: $path" >&2; exit 1;
    }
    queue_manifest "$nearest"
  done < "$index_list"
  removed_owners+=("$owner")
}
while IFS= read -r -d '' staged_status && IFS= read -r -d '' staged_path; do
  if [[ "$staged_status" != D ]]; then
    if git check-ignore --no-index -q -- "$staged_path"; then
      printf 'error: gitignored file must not be committed: %q\n' "$staged_path" >&2
      exit 1
    else
      ignore_status=$?
      [[ "$ignore_status" == 1 ]] || exit "$ignore_status"
    fi
  fi
  [[ "$staged_path" == *.rs ]] || continue
  if [[ "$staged_status" == D ]]; then
    load_head
    original=$(find_head_manifest "$staged_path") || {
      echo "no original Cargo owner for deleted Rust file: $staged_path" >&2; exit 1;
    }
    if index_has "$original"; then queue_manifest "$original"; else prove_removed_owner "$original"; fi
  else
    owner=$(find_index_manifest "$staged_path") || {
      owner_status=$?; [[ "$owner_status" == 1 ]] || exit "$owner_status"
      echo "no indexed Cargo owner for staged Rust file: $staged_path" >&2; exit 1;
    }
    queue_manifest "$owner"
  fi
done < "$staged_list"
for ((index=0; index<count; index++)); do
  cargo fmt --manifest-path "${manifests[index]}" --package "${packages[index]}" -- --check
done
for ((index=0; index<count; index++)); do
  bash "$root/scripts/dev/build-budget.sh" --lane pre-commit -- \
    cargo check --locked --manifest-path "${manifests[index]}" --package "${packages[index]}" --quiet
done
