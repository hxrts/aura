#!/usr/bin/env bash
# Isolated real-SSH fixture. Point REMOTE_HOST at an already authenticated
# Linux host and REMOTE_BASE at an existing private directory on its ext4 disk.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
host="${AURA_COLD_CACHE_TEST_REMOTE_HOST:?set authenticated SSH host}"
base="${AURA_COLD_CACHE_TEST_REMOTE_BASE:?set private ext4 directory}"
[[ "$host" =~ ^[A-Za-z0-9_.-]+@[A-Za-z0-9_.-]+$ &&
   "$base" =~ ^/[A-Za-z0-9_./-]+$ && "$base" != *'/../'* ]] || exit 2
ssh_safe() {
  ssh -T -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=10 "$host" "$@"
}
stat_mode() {
  local result
  result="$(stat -f %Lp "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-7]{3,4}$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %a "$1"
}
stat_mtime() {
  local result
  result="$(stat -f %m "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-9]+$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %Y "$1"
}
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-cold-remote-test.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
remote="$(ssh_safe "mktemp -d -p $base aura-cold-remote-test.XXXXXX")"
[[ "$remote" == "$base"/aura-cold-remote-test.* ]] || exit 1
cleanup() {
  ssh_safe "rm -rf -- $remote" || true
  rm -rf -- "$fixture"
}
trap cleanup EXIT
project="$fixture/project"
mkdir -p "$project/target/dylint" "$fixture/shared"
git init -q "$project"
printf '[package]\nname="cold-cache-remote-fixture"\nversion="0.1.0"\n' > "$project/Cargo.toml"
printf '/target/\n' > "$project/.gitignore"
git -C "$project" add Cargo.toml .gitignore
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
printf 'preserved compiler artifact\n' > "$project/target/dylint/cache"
chmod 640 "$project/target/dylint/cache"
touch -t 202001020304 "$project/target/dylint/cache"
original_mode="$(stat_mode "$project/target/dylint/cache")"
original_mtime="$(stat_mtime "$project/target/dylint/cache")"
export AURA_COLD_CACHE_REMOTE_HOST="$host" AURA_COLD_CACHE_REMOTE_DIR="$remote"
export AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=0
export AURA_COLD_CACHE_REMOTE_MIN_FREE_GIB=0
unset AURA_COLD_CACHE_STORE
cold() { bash "$repo_root/scripts/dev/cold-cache-store.sh" --root "$project" --lane "$1" "$2"; }
expect_fail() {
  if cold "$@" > "$fixture/output" 2>&1; then
    echo "unexpected remote cold-store success: $*" >&2; exit 1
  fi
}
chmod 755 "$remote" 2>/dev/null || ssh_safe "chmod 755 $remote"
expect_fail dylint --export
[[ -f "$project/target/dylint/cache" ]]
ssh_safe "chmod 700 $remote"
AURA_COLD_CACHE_REMOTE_MIN_FREE_GIB=99999 expect_fail dylint --export
[[ -f "$project/target/dylint/cache" ]]
remote_machine="$(ssh_safe 'cat /etc/machine-id')"
mkdir -p "$fixture/fakebin"
cat > "$fixture/fakebin/uname" <<'FAKE_UNAME'
#!/usr/bin/env bash
if [[ "$1" == -s ]]; then printf 'Linux\n'; else exec /usr/bin/uname "$@"; fi
FAKE_UNAME
cat > "$fixture/fakebin/cat" <<'FAKE_CAT'
#!/usr/bin/env bash
if [[ "$1" == /etc/machine-id ]]; then
  printf '%s\n' "$AURA_FAKE_LOCAL_MACHINE"
else exec /bin/cat "$@"; fi
FAKE_CAT
chmod +x "$fixture/fakebin/uname" "$fixture/fakebin/cat"
AURA_FAKE_LOCAL_MACHINE="$remote_machine" PATH="$fixture/fakebin:$PATH" \
  expect_fail dylint --export
rg -q 'not independently identified from Linux source' "$fixture/output"
[[ -f "$project/target/dylint/cache" ]]
rm "$fixture/fakebin/uname" "$fixture/fakebin/cat"
mkfifo "$project/target/dylint/special"
expect_fail dylint --export
[[ -f "$project/target/dylint/cache" ]]
rm "$project/target/dylint/special"
cold dylint --export > "$fixture/output"
[[ ! -e "$project/target/dylint" ]]
revision="$(git -C "$project" rev-parse HEAD)"
key="$(printf '%s' "$project" | sha256sum | awk '{print $1}')"
ready="$remote/aura-cold-cache/$key/$revision/dylint/ready"
receipt="$fixture/shared/cold-remote-receipts/aura-cold-cache/$key/$revision/dylint"
[[ -f "$receipt/location.json" && -f "$receipt/location.sig" ]]
ssh_safe "set -e; test \"\$(stat -c %a $ready)\" = 700; test \"\$(stat -c %a $ready/archive.tar)\" = 600; test -f $ready/location.json; test -f $ready/location.sig"
cmp "$receipt/location.json" <(ssh_safe "cat $ready/location.json")
cmp "$receipt/location.sig" <(ssh_safe "cat $ready/location.sig")
jq -e --arg host "$host" --arg dir "$remote" \
  '.remote_host==$host and .remote_dir==$dir and (.rollback|type=="string")' \
  "$receipt/location.json" >/dev/null
mkdir -p "$project/target/dylint"
printf 'existing artifact\n' > "$project/target/dylint/cache"
expect_fail dylint --restore
[[ "$(cat "$project/target/dylint/cache")" == 'existing artifact' ]]
expect_fail dylint --export
[[ -f "$project/target/dylint/cache" ]]
rm -rf "$project/target/dylint"
archive_bytes="$(jq -r '.bytes' "$receipt/location.json")"
allocated_kib="$(jq -r '.source_allocated_kib' "$receipt/location.json")"
archive_kib=$(( (archive_bytes + 1023) / 1024 ))
extract_kib="$allocated_kib"
if (( archive_kib > extract_kib )); then extract_kib="$archive_kib"; fi
one_copy_free=$(( archive_kib + 1024 * 1024 + 1 ))
(( one_copy_free < archive_kib + extract_kib + 1024 * 1024 ))
mkdir -p "$fixture/fakebin"
cat > "$fixture/fakebin/df" <<'FAKE_DF'
#!/usr/bin/env bash
if [[ "$1" == -Pk && "$2" == "$AURA_FAKE_SOURCE_ROOT" ]]; then
  printf 'Filesystem 1024-blocks Used Available Capacity Mounted on\n'
  printf '/dev/fake 9999999 1 %s 1%% /source\n' "$AURA_FAKE_FREE_KIB"
else exec /bin/df "$@"; fi
FAKE_DF
chmod +x "$fixture/fakebin/df"
AURA_FAKE_SOURCE_ROOT="$project" AURA_FAKE_FREE_KIB="$one_copy_free" \
  PATH="$fixture/fakebin:$PATH" expect_fail dylint --restore
[[ ! -e "$project/target/dylint" ]]
cat > "$fixture/fakebin/tar" <<'FAKE_TAR'
#!/usr/bin/env bash
mode() {
  local result
  result="$(stat -f %Lp "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-7]{3,4}$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %a "$1"
}
if [[ " $* " == *' -pxf '* ]]; then
  [[ "$(mode "$(dirname "$2")")" == 700 &&
     "$(mode "$2")" == 700 ]] || exit 89
  chmod 755 "$2" # archived '.' may make the payload readable mid-extraction
  printf 'partial extraction\n' > "$2/partial"
  exit 88
fi
exec /usr/bin/tar "$@"
FAKE_TAR
chmod +x "$fixture/fakebin/tar"
PATH="$fixture/fakebin:$PATH" expect_fail dylint --restore
[[ ! -e "$project/target/dylint" &&
   -z "$(find "$project/target" -maxdepth 1 -name '.cold-restore.*' -print -quit)" ]]
rm "$fixture/fakebin/tar" "$fixture/fakebin/df"
cold dylint --restore > "$fixture/output"
[[ "$(cat "$project/target/dylint/cache")" == 'preserved compiler artifact' ]]
[[ "$(stat_mode "$project/target/dylint/cache")" == "$original_mode" &&
   "$(stat_mtime "$project/target/dylint/cache")" == "$original_mtime" ]]
rm -rf "$project/target/dylint"
printf '# changed revision\n' >> "$project/Cargo.toml"
git -C "$project" add Cargo.toml
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm changed
expect_fail dylint --restore
[[ ! -e "$project/target/dylint" ]]
git -C "$project" reset --hard -q "$revision"
cp "$receipt/location.json" "$fixture/location-original.json"
printf ' ' >> "$receipt/location.json"
expect_fail dylint --restore
[[ ! -e "$project/target/dylint" ]]
cp "$fixture/location-original.json" "$receipt/location.json"
ssh_safe "chmod 644 $ready/archive.tar"
expect_fail dylint --restore
[[ ! -e "$project/target/dylint" ]]
ssh_safe "chmod 600 $ready/archive.tar"
ssh_safe "printf tamper >> $ready/archive.tar"
expect_fail dylint --restore
[[ ! -e "$project/target/dylint" ]]
# A failed upload leaves the original lane and a non-ready pending snapshot;
# subsequent export refuses that interrupted snapshot rather than replacing it.
mkdir -p "$project/target/wasm32-unknown-unknown/debug" "$fixture/fakebin"
printf 'other artifact\n' > "$project/target/wasm32-unknown-unknown/debug/cache"
cat > "$fixture/fakebin/ssh" <<'FAKE_SSH'
#!/usr/bin/env bash
if [[ "$*" == *'cat > '*'archive.tar'* ]]; then exit 22; fi
exec /usr/bin/ssh "$@"
FAKE_SSH
chmod +x "$fixture/fakebin/ssh"
PATH="$fixture/fakebin:$PATH" expect_fail wasm-debug --export
[[ -f "$project/target/wasm32-unknown-unknown/debug/cache" ]]
ssh_safe "test -d $remote/aura-cold-cache/$key/$revision/wasm-debug/.pending; test ! -e $remote/aura-cold-cache/$key/$revision/wasm-debug/ready"
expect_fail wasm-debug --export
[[ -f "$project/target/wasm32-unknown-unknown/debug/cache" ]]
mkdir -p "$project/target/wasm32-unknown-unknown/wasm"
printf 'pending-location artifact\n' > "$project/target/wasm32-unknown-unknown/wasm/cache"
cat > "$fixture/fakebin/ssh" <<'FAKE_SSH'
#!/usr/bin/env bash
if [[ "$*" == *'cat > '*'location.sig'* ]]; then exit 23; fi
exec /usr/bin/ssh "$@"
FAKE_SSH
chmod +x "$fixture/fakebin/ssh"
PATH="$fixture/fakebin:$PATH" expect_fail wasm-profile --export
[[ -f "$project/target/wasm32-unknown-unknown/wasm/cache" ]]
ssh_safe "test -d $remote/aura-cold-cache/$key/$revision/wasm-profile/.pending; test ! -e $remote/aura-cold-cache/$key/$revision/wasm-profile/ready"
rm "$fixture/fakebin/ssh"
mkdir -p "$project/target/wasm"
printf 'locked export artifact\n' > "$project/target/wasm/cache"
ssh_safe "mkdir -m 700 $remote/aura-cold-cache/.capacity.lock; printf stale > $remote/aura-cold-cache/.capacity.lock/token"
expect_fail wasm-root-profile --export
[[ -f "$project/target/wasm/cache" ]]
ssh_safe "rm $remote/aura-cold-cache/.capacity.lock/token; rmdir $remote/aura-cold-cache/.capacity.lock"
mkdir -p "$project/target/release"
printf 'final-source artifact\n' > "$project/target/release/cache"
cat > "$fixture/fakebin/ssh" <<'FAKE_SSH'
#!/usr/bin/env bash
if [[ "$*" == *'/release/ready/archive.tar'* && "$*" == *'cat '* ]]; then
  printf '# changed while exporting\n' >> "$AURA_FAKE_SOURCE_ROOT/Cargo.toml"
fi
exec /usr/bin/ssh "$@"
FAKE_SSH
chmod +x "$fixture/fakebin/ssh"
AURA_FAKE_SOURCE_ROOT="$project" PATH="$fixture/fakebin:$PATH" expect_fail release --export
[[ -f "$project/target/release/cache" ]]
rg -q 'source changed before guarded prune' "$fixture/output"
echo 'cold remote: authenticated ext4, permissions, signed roundtrip, occupied/overwrite, capacity, tamper, and interrupted upload refusals passed'
