#!/usr/bin/env bash
# Reject ignored staged paths and check each actual package owning changed Rust.
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
staged_list=$(mktemp)
trap 'rm -f "$staged_list"' EXIT
# Enumerate a rename's source and destination so both Cargo owners are checked.
git diff --no-relative --no-renames --cached --name-only --diff-filter=ACMRD -z > "$staged_list"
while IFS= read -r -d '' staged_path; do
  if git check-ignore --no-index -q -- "$staged_path"; then
    printf 'error: gitignored file must not be committed: %q\n' "$staged_path" >&2
    exit 1
  else
    ignore_status=$?
    [[ "$ignore_status" == 1 ]] || exit "$ignore_status"
  fi
  [[ "$staged_path" == *.rs ]] || continue
  directory=$(dirname "$root/$staged_path")
  while [[ "$directory" != "$root" && ! -f "$directory/Cargo.toml" ]]; do
    directory=$(dirname "$directory")
  done
  manifest="$directory/Cargo.toml"
  [[ -f "$manifest" ]] || {
    echo "no owning Cargo manifest for staged Rust file: $staged_path" >&2
    exit 1
  }
  duplicate=false
  for ((index=0; index<count; index++)); do
    if [[ "${manifests[index]}" == "$manifest" ]]; then duplicate=true; break; fi
  done
  [[ "$duplicate" == false ]] || continue
  metadata=$(cargo metadata --no-deps --locked --format-version 1 --manifest-path "$manifest")
  package=$(jq -er --arg manifest "$manifest" '
    [.packages[] | select(.manifest_path == $manifest)] |
    if length == 1 then .[0].name else error("staged Rust file has no unique owning Cargo package") end
  ' <<< "$metadata")
  manifests[count]=$manifest
  packages[count]=$package
  count=$((count + 1))
done < "$staged_list"
for ((index=0; index<count; index++)); do
  cargo fmt --manifest-path "${manifests[index]}" --package "${packages[index]}" -- --check
done
for ((index=0; index<count; index++)); do
  bash "$root/scripts/dev/build-budget.sh" --lane pre-commit -- \
    cargo check --locked --manifest-path "${manifests[index]}" --package "${packages[index]}" --quiet
done
