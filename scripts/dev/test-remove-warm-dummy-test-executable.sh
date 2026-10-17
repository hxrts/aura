#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd -P)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-warm-dummy.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
chmod 700 "$fixture"
mkdir -m 700 "$fixture/bin" "$fixture/target" "$fixture/target/debug" "$fixture/target/debug/deps"
cat > "$fixture/bin/cargo" <<'CARGO'
#!/bin/bash
[[ "$1" == metadata ]] || exit 2
cat "$AURA_TEST_METADATA"
CARGO
chmod 700 "$fixture/bin/cargo"
printf '%s\n' '{"packages":[{"name":"hxrts-aura-core","id":"path+file:///dummy#hxrts-aura-core@0.2.0"}]}' > "$fixture/metadata.json"
printf '%s\n' 'manifest' > "$fixture/Cargo.toml"
export AURA_TEST_METADATA="$fixture/metadata.json"
runner="$repo/scripts/dev/remove-warm-dummy-test-executable.sh"
exe="$fixture/target/debug/deps/aura_core-0123456789abcdef"
external="$fixture/target/debug/deps/foreign-0123456789abcdef"
fail() { printf '%s\n' "warm dummy fixture: $1" >&2; exit 1; }
artifact() {
  jq -nc --arg package "$1" --arg executable "$2" --arg name "${3:-aura_core}" --arg kind "${4:-lib}" '{reason:"compiler-artifact",package_id:$package,target:{name:$name,kind:[$kind]},profile:{test:true},executable:$executable}'
}
reset_files() {
  printf '%s\n' dummy > "$exe"
  printf '%s\n' external > "$external"
  chmod 700 "$exe" "$external"
}
accept() {
  PATH="$fixture/bin:$PATH" bash "$runner" "$fixture/artifacts.jsonl" "$fixture/target" "$fixture/Cargo.toml" > "$fixture/out" 2>&1 || fail 'valid local identity refused'
  [[ ! -e "$exe" && -f "$external" ]] || fail 'wrong executable removed'
}
refuse() {
  local result=0
  PATH="$fixture/bin:$PATH" bash "$runner" "$fixture/artifacts.jsonl" "$fixture/target" "$fixture/Cargo.toml" > "$fixture/out" 2>&1 || result=$?
  [[ "$result" == 1 && -f "$external" ]] || fail "unexpected refusal result $result or external file lost"
}

reset_files
artifact 'path+file:///dummy#hxrts-aura-core@0.2.0' "$exe" > "$fixture/artifacts.jsonl"
artifact 'registry+foreign#other@1.0.0' "$external" >> "$fixture/artifacts.jsonl"
accept
reset_files
cat "$fixture/artifacts.jsonl" "$fixture/artifacts.jsonl" > "$fixture/duplicate.jsonl"
cp "$fixture/duplicate.jsonl" "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" ]] || fail 'duplicate identity removed file'
reset_files
artifact 'path+file:///dummy#hxrts-aura-core@0.2.0' "$exe" > "$fixture/artifacts.jsonl"
artifact 'path+file:///dummy#hxrts-aura-core@0.2.0' "$external" extra_core bin >> "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" && -f "$external" ]] || fail 'unexpected second local executable changed files'
reset_files
artifact 'registry+foreign#other@1.0.0' "$exe" > "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" ]] || fail 'foreign identity removed file'
reset_files
true > "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" ]] || fail 'empty artifact log removed file'
reset_files
printf '%s\n' '{bad-json' > "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" ]] || fail 'malformed artifact log removed file'
reset_files
artifact 'path+file:///dummy#hxrts-aura-core@0.2.0' "$exe" aura_core bin > "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" ]] || fail 'wrong target kind removed file'
reset_files
artifact 'path+file:///dummy#hxrts-aura-core@0.2.0' "$exe" wrong_core lib > "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" ]] || fail 'wrong target name removed file'
reset_files
artifact 'path+file:///dummy#hxrts-aura-core@0.2.0' "$exe" | jq -c '.profile.test=false' > "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" ]] || fail 'non-test artifact removed file'
reset_files
artifact 'path+file:///dummy#hxrts-aura-core@0.2.0' "$fixture/escape/aura_core-0123456789abcdef" > "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" ]] || fail 'foreign path removed local file'
reset_files
mv "$fixture/target/debug" "$fixture/target/debug-real"
ln -s debug-real "$fixture/target/debug"
artifact 'path+file:///dummy#hxrts-aura-core@0.2.0' "$exe" > "$fixture/artifacts.jsonl"
refuse
[[ -f "$exe" ]] || fail 'symlink ancestor removed file'
rm "$fixture/target/debug"
mv "$fixture/target/debug-real" "$fixture/target/debug"
reset_files
rm "$exe"
ln -s "$external" "$exe"
artifact 'path+file:///dummy#hxrts-aura-core@0.2.0' "$exe" > "$fixture/artifacts.jsonl"
refuse
[[ -L "$exe" && -f "$external" ]] || fail 'symlink refusal changed files'
rm "$exe"
reset_files
chmod 600 "$exe"
refuse
[[ -f "$exe" ]] || fail 'non-executable refusal removed file'
reset_files
printf '%s\n' '{"packages":[{"name":"hxrts-aura-core","id":"first"},{"name":"hxrts-aura-core","id":"second"}]}' > "$fixture/metadata.json"
refuse
[[ -f "$exe" ]] || fail 'ambiguous Cargo metadata removed file'
printf '%s\n' 'warm dummy executable classification tests passed'
