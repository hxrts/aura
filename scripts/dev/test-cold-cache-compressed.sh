#!/usr/bin/env bash
# Authenticated remote gzip archive contract; uses only isolated source trees.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
host="${AURA_COLD_CACHE_TEST_REMOTE_HOST:?set authenticated SSH host}"
base="${AURA_COLD_CACHE_TEST_REMOTE_BASE:?set private ext4 directory}"
[[ "$host" =~ ^[A-Za-z0-9_.-]+@[A-Za-z0-9_.-]+$ &&
   "$base" =~ ^/[A-Za-z0-9_./-]+$ && "$base" != *'/../'* ]] || exit 2
ssh_safe() { ssh -T -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=10 "$host" "$@"; }
stat_mode() {
  local result
  result="$(stat -f %Lp "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-7]{3,4}$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %a "$1"
}
stat_size() {
  local result
  result="$(stat -f %z "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-9]+$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %s "$1"
}
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-cold-compressed-test.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
remote="$(ssh_safe "mktemp -d -p $base aura-cold-compressed-test.XXXXXX")"
[[ "$remote" == "$base"/aura-cold-compressed-test.* ]] || exit 1
cleanup() {
  local status=$?
  if (( status != 0 )) && [[ -f "$fixture/output" ]]; then
    printf 'compressed fixture first failure (status %s):\n' "$status" >&2
    cat "$fixture/output" >&2
  fi
  ssh_safe "rm -rf -- $remote" || true
  rm -rf -- "$fixture"
  return "$status"
}
trap cleanup EXIT
project="$fixture/project"
mkdir -p "$project/target/dylint" "$fixture/shared"
git init -q "$project"
printf '[package]\nname="cold-compressed-fixture"\nversion="0.1.0"\n' > "$project/Cargo.toml"
printf '/target/\n' > "$project/.gitignore"
git -C "$project" add Cargo.toml .gitignore
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
printf 'compressed compiler artifact\n' > "$project/target/dylint/cache"
chmod 640 "$project/target/dylint/cache"
touch -t 202001020304 "$project/target/dylint/cache"
export AURA_COLD_CACHE_REMOTE_HOST="$host" AURA_COLD_CACHE_REMOTE_DIR="$remote"
export AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=0
export AURA_COLD_CACHE_REMOTE_MIN_FREE_GIB=0
unset AURA_COLD_CACHE_STORE
cold() { bash "$repo_root/scripts/dev/cold-cache-store.sh" --root "$project" --lane "$1" "${@:2}"; }
expect_fail() {
  if cold "$@" > "$fixture/output" 2>&1; then
    echo "unexpected compressed cold-store success: $*" >&2; exit 1
  fi
}
ln -s cache "$project/target/dylint/linked"
expect_fail dylint --export --compression gzip-1
rg -q 'linked artifact' "$fixture/output"
rm "$project/target/dylint/linked"
ln "$project/target/dylint/cache" "$project/target/dylint/linked"
expect_fail dylint --export --compression gzip-1
rg -q 'linked artifact' "$fixture/output"
rm "$project/target/dylint/linked"
exec 9< "$project/target/dylint/cache"
expect_fail dylint --export --compression gzip-1
rg -q 'lane has open files' "$fixture/output"
exec 9<&-
mkdir -p "$project/target/wasm" "$fixture/fakebin"
printf 'failed inventory source\n' > "$project/target/wasm/cache"
export AURA_REAL_SHA256SUM="$(command -v sha256sum)"
export AURA_FAKE_HASH_FAILURE_PATH="$project/target/wasm/cache"
cat > "$fixture/fakebin/sha256sum" <<'FAKE_HASH'
#!/usr/bin/env bash
[[ "${1:-}" != "$AURA_FAKE_HASH_FAILURE_PATH" ]] || exit 83
exec "$AURA_REAL_SHA256SUM" "$@"
FAKE_HASH
chmod +x "$fixture/fakebin/sha256sum"
PATH="$fixture/fakebin:$PATH" expect_fail wasm-root-profile --export --compression gzip-1
rg -q 'source member inventory failed' "$fixture/output"
[[ -f "$project/target/wasm/cache" ]]
rm "$fixture/fakebin/sha256sum"
cold dylint --export --compression gzip-1 > "$fixture/output"
revision="$(git -C "$project" rev-parse HEAD)"
key="$(printf '%s' "$project" | sha256sum | awk '{print $1}')"
ready="$remote/aura-cold-cache/$key/$revision/dylint/ready"
receipt="$fixture/shared/cold-remote-receipts/aura-cold-cache/$key/$revision/dylint"
signing_key="$fixture/shared/cold-keys/$key.pem"
[[ ! -e "$project/target/dylint" && -f "$receipt/location.sig" ]]
ssh_safe "test -f $ready/archive.tar.gz; test ! -e $ready/archive.tar; test -f $ready/members.tsv"
jq -e '.schema==2 and .compression=="gzip-1" and .tar_bytes>0 and
  (.metadata_sha256|test("^[0-9a-f]{64}$")) and .source_relative==null' \
  "$receipt/location.json" >/dev/null
cold dylint --restore > "$fixture/output"
[[ "$(cat "$project/target/dylint/cache")" == 'compressed compiler artifact' ]]
[[ "$(stat_mode "$project/target/dylint/cache")" == 640 ]]
rm -rf "$project/target/dylint"
printf '# wrong source\n' >> "$project/Cargo.toml"
git -C "$project" add Cargo.toml
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm changed
expect_fail dylint --restore
[[ ! -e "$project/target/dylint" ]]
git -C "$project" reset --hard -q "$revision"
ssh_safe "cat $ready/members.tsv" > "$fixture/members.original"
ssh_safe "printf x >> $ready/members.tsv"
expect_fail dylint --restore
rg -q 'metadata manifest mismatch' "$fixture/output"
[[ ! -e "$project/target/dylint" ]]
# Return only this isolated fixture's original authenticated metadata.
ssh_safe "cat > $ready/members.tsv" < "$fixture/members.original"
cp "$receipt/location.json" "$fixture/location.original"
ssh_safe "cat $ready/manifest.json" > "$fixture/manifest.original"
jq '.gzip_sha256=("0"*64)' "$receipt/location.json" > "$fixture/location.wrong-tool"
jq '.gzip_sha256=("0"*64)' "$fixture/manifest.original" > "$fixture/manifest.wrong-tool"
chmod 600 "$fixture/location.wrong-tool" "$fixture/manifest.wrong-tool"
mv "$fixture/location.wrong-tool" "$receipt/location.json"
openssl dgst -sha256 -sign "$signing_key" -out "$receipt/location.sig" "$receipt/location.json"
openssl dgst -sha256 -sign "$signing_key" -out "$fixture/manifest.wrong-tool.sig" "$fixture/manifest.wrong-tool"
ssh_safe "cat > $ready/location.json" < "$receipt/location.json"
ssh_safe "cat > $ready/location.sig" < "$receipt/location.sig"
ssh_safe "cat > $ready/manifest.json" < "$fixture/manifest.wrong-tool"
ssh_safe "cat > $ready/manifest.sig" < "$fixture/manifest.wrong-tool.sig"
expect_fail dylint --restore
rg -q 'pinned gzip identity changed' "$fixture/output"
[[ ! -e "$project/target/dylint" ]]
cp "$fixture/location.original" "$receipt/location.json"
openssl dgst -sha256 -sign "$signing_key" -out "$receipt/location.sig" "$receipt/location.json"
ssh_safe "cat > $ready/location.json" < "$receipt/location.json"
ssh_safe "cat > $ready/location.sig" < "$receipt/location.sig"
ssh_safe "cat > $ready/manifest.json" < "$fixture/manifest.original"
openssl dgst -sha256 -sign "$signing_key" -out "$fixture/manifest.original.sig" "$fixture/manifest.original"
ssh_safe "cat > $ready/manifest.sig" < "$fixture/manifest.original.sig"
# A legitimately signed but too-small expansion declaration must stop at the
# size boundary before any staged extraction or target publication.
jq '.tar_bytes=1' "$receipt/location.json" > "$fixture/location.small"
jq '.tar_bytes=1' "$fixture/manifest.original" > "$fixture/manifest.small"
chmod 600 "$fixture/location.small" "$fixture/manifest.small"
mv "$fixture/location.small" "$receipt/location.json"
openssl dgst -sha256 -sign "$signing_key" -out "$receipt/location.sig" "$receipt/location.json"
openssl dgst -sha256 -sign "$signing_key" -out "$fixture/manifest.small.sig" "$fixture/manifest.small"
ssh_safe "cat > $ready/location.json" < "$receipt/location.json"
ssh_safe "cat > $ready/location.sig" < "$receipt/location.sig"
ssh_safe "cat > $ready/manifest.json" < "$fixture/manifest.small"
ssh_safe "cat > $ready/manifest.sig" < "$fixture/manifest.small.sig"
expect_fail dylint --restore
rg -q 'compressed expansion exceeds signed bound' "$fixture/output" || {
  cat "$fixture/output" >&2; exit 1;
}
[[ ! -e "$project/target/dylint" &&
   -z "$(find "$project/target" -maxdepth 1 -name '.cold-restore.*' -print -quit)" ]]
cp "$fixture/location.original" "$receipt/location.json"
openssl dgst -sha256 -sign "$signing_key" -out "$receipt/location.sig" "$receipt/location.json"
ssh_safe "cat > $ready/location.json" < "$receipt/location.json"
ssh_safe "cat > $ready/location.sig" < "$receipt/location.sig"
ssh_safe "cat > $ready/manifest.json" < "$fixture/manifest.original"
openssl dgst -sha256 -sign "$signing_key" -out "$fixture/manifest.original.sig" "$fixture/manifest.original"
ssh_safe "cat > $ready/manifest.sig" < "$fixture/manifest.original.sig"
# A valid signature over corrupted gzip bytes must still reject the decoder's
# nonzero CRC status, even if it emitted the original logical tar first.
ssh_safe "cat $ready/archive.tar.gz" > "$fixture/archive.original"
cp "$fixture/archive.original" "$fixture/archive.bad"
bad_offset=$(( $(wc -c < "$fixture/archive.bad") - 8 ))
original_crc_byte="$(od -An -tu1 -j "$bad_offset" -N 1 "$fixture/archive.bad" | tr -d ' ')"
if [[ "$original_crc_byte" == 255 ]]; then replacement='\000'; else replacement='\377'; fi
printf "$replacement" | dd of="$fixture/archive.bad" bs=1 seek="$bad_offset" conv=notrunc 2>/dev/null
bad_sha="$(sha256sum "$fixture/archive.bad" | awk '{print $1}')"
[[ "$bad_sha" != "$(sha256sum "$fixture/archive.original" | awk '{print $1}')" ]]
jq --arg sha "$bad_sha" '.sha256=$sha' "$receipt/location.json" > "$fixture/location.bad"
jq --arg sha "$bad_sha" '.sha256=$sha' "$fixture/manifest.original" > "$fixture/manifest.bad"
chmod 600 "$fixture/location.bad" "$fixture/manifest.bad"
mv "$fixture/location.bad" "$receipt/location.json"
openssl dgst -sha256 -sign "$signing_key" -out "$receipt/location.sig" "$receipt/location.json"
openssl dgst -sha256 -sign "$signing_key" -out "$fixture/manifest.bad.sig" "$fixture/manifest.bad"
ssh_safe "cat > $ready/location.json" < "$receipt/location.json"
ssh_safe "cat > $ready/location.sig" < "$receipt/location.sig"
ssh_safe "cat > $ready/manifest.json" < "$fixture/manifest.bad"
ssh_safe "cat > $ready/manifest.sig" < "$fixture/manifest.bad.sig"
ssh_safe "cat > $ready/archive.tar.gz" < "$fixture/archive.bad"
expect_fail dylint --restore
rg -q 'compressed stream integrity failed' "$fixture/output" || {
  cat "$fixture/output" >&2; exit 1;
}
[[ ! -e "$project/target/dylint" ]]
cp "$fixture/location.original" "$receipt/location.json"
openssl dgst -sha256 -sign "$signing_key" -out "$receipt/location.sig" "$receipt/location.json"
ssh_safe "cat > $ready/location.json" < "$receipt/location.json"
ssh_safe "cat > $ready/location.sig" < "$receipt/location.sig"
ssh_safe "cat > $ready/manifest.json" < "$fixture/manifest.original"
ssh_safe "cat > $ready/manifest.sig" < "$fixture/manifest.original.sig"
ssh_safe "truncate -s 4 $ready/archive.tar.gz"
expect_fail dylint --restore
rg -q 'remote archive hash or size mismatch' "$fixture/output"
[[ ! -e "$project/target/dylint" ]]
mkdir -p "$project/target/wasm32-unknown-unknown/debug" "$fixture/fakebin"
printf 'interrupted compiler lane\n' > "$project/target/wasm32-unknown-unknown/debug/cache"
cat > "$fixture/fakebin/ssh" <<'FAKE_SSH'
#!/usr/bin/env bash
if [[ "$*" == *'cat > '*'archive.tar.gz'* ]]; then exit 22; fi
exec /usr/bin/ssh "$@"
FAKE_SSH
chmod +x "$fixture/fakebin/ssh"
PATH="$fixture/fakebin:$PATH" expect_fail wasm-debug --export --compression gzip-1
[[ -f "$project/target/wasm32-unknown-unknown/debug/cache" ]]
ssh_safe "test -d $remote/aura-cold-cache/$key/$revision/wasm-debug/.pending; test ! -e $remote/aura-cold-cache/$key/$revision/wasm-debug/ready"
rm "$fixture/fakebin/ssh"
mkdir -p "$project/target/debug"
printf 'second compiler lane\n' > "$project/target/debug/cache"
truncate -s 67108864 "$project/target/debug/sparse"
cold debug --export --compression gzip-1 > "$fixture/output"
[[ ! -e "$project/target/debug" ]]
debug_receipt="$fixture/shared/cold-remote-receipts/aura-cold-cache/$key/$revision/debug/location.json"
debug_tar_kib=$(( ($(jq -r '.tar_bytes' "$debug_receipt") + 1023) / 1024 ))
debug_allocated_kib="$(jq -r '.source_allocated_kib' "$debug_receipt")"
debug_archive_kib=$(( ($(jq -r '.bytes' "$debug_receipt") + 1023) / 1024 ))
(( debug_tar_kib > debug_allocated_kib )) || {
  echo 'sparse fixture did not exceed allocated-size reservation' >&2; exit 1;
}
cat > "$fixture/fakebin/df" <<'FAKE_DF'
#!/usr/bin/env bash
if [[ "$1" == -Pk && "$2" == "$AURA_FAKE_SOURCE_ROOT" ]]; then
  printf 'Filesystem 1024-blocks Used Available Capacity Mounted on\n'
  printf '/dev/fake 9999999 1 %s 1%% /source\n' "$AURA_FAKE_FREE_KIB"
else exec /bin/df "$@"; fi
FAKE_DF
chmod +x "$fixture/fakebin/df"
one_copy_free=$(( debug_archive_kib + debug_allocated_kib + 1024 * 1024 + 1 ))
AURA_FAKE_SOURCE_ROOT="$project" AURA_FAKE_FREE_KIB="$one_copy_free" \
  PATH="$fixture/fakebin:$PATH" expect_fail debug --restore
rg -q 'remote restore would breach admission floor' "$fixture/output"
[[ ! -e "$project/target/debug" ]]
rm "$fixture/fakebin/df"
cold debug --restore > "$fixture/output"
[[ "$(cat "$project/target/debug/cache")" == 'second compiler lane' &&
   "$(stat_size "$project/target/debug/sparse")" == 67108864 ]]
echo 'cold compressed remote: signed dylint/debug roundtrips, logical-tar floor, bounded expansion, tool/CRC/truncation, wrong source, links/open file, and interrupted upload refusals passed'
