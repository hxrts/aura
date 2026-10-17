#!/usr/bin/env bash
# Omit only the dummy local test executable from Crane's dependency artifacts.
set -euo pipefail

[[ $# == 3 ]] || { echo 'warm dummy executable: expected artifact log, target root, manifest' >&2; exit 2; }
artifact_log=$1
target_root=$2
manifest=$3
[[ -f "$artifact_log" && ! -L "$artifact_log" && -d "$target_root" && ! -L "$target_root" && -f "$manifest" ]] || {
  echo 'warm dummy executable: missing or unsafe input' >&2; exit 2;
}

metadata=$(cargo metadata --offline --locked --no-deps --format-version=1 --manifest-path "$manifest") || {
  echo 'warm dummy executable: Cargo metadata failed' >&2; exit 1;
}
package_id=$(jq -er '[.packages[] | select(.name == "hxrts-aura-core") | .id] | if length == 1 then .[0] else error("core package identity ambiguous") end' <<< "$metadata") || {
  echo 'warm dummy executable: core package identity refused' >&2; exit 1;
}

# Cargo, rather than a filename glob, identifies the one local lib-test output.
executable=$(jq -ser --arg package_id "$package_id" '
  [ .[] | select(.reason == "compiler-artifact" and .package_id == $package_id
    and (.executable | type == "string")) ]
  | if length == 1 and .[0].target.name == "aura_core"
      and .[0].target.kind == ["lib"] and .[0].profile.test == true
    then .[0].executable else error("dummy test executable identity ambiguous") end
' "$artifact_log") || {
  echo 'warm dummy executable: Cargo artifact identity refused' >&2; exit 1;
}
deps_root="$target_root/debug/deps"
[[ -d "$target_root/debug" && ! -L "$target_root/debug" &&
   -d "$deps_root" && ! -L "$deps_root" && "${executable%/*}" == "$deps_root" &&
   -f "$executable" && ! -L "$executable" && -x "$executable" ]] || {
  echo 'warm dummy executable: output path or file refused' >&2; exit 1;
}
[[ "${executable##*/}" =~ ^aura_core-[0-9a-f]{16}$ ]] || {
  echo 'warm dummy executable: unexpected Cargo executable name' >&2; exit 1;
}

rm -- "$executable"
echo 'warm dummy executable: omitted one Cargo-identified local test binary' >&2
