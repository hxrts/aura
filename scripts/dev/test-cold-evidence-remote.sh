#!/usr/bin/env bash
# Exact protected-run archive/restore fixture on the existing independent SSH ext4 backend.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
host="${AURA_COLD_CACHE_TEST_REMOTE_HOST:?authenticated remote host required}"
base="${AURA_COLD_CACHE_TEST_REMOTE_BASE:?private ext4 base required}"
[[ "$host" =~ ^[A-Za-z0-9_.-]+@[A-Za-z0-9_.-]+$ &&
   "$base" =~ ^/[A-Za-z0-9_./-]+$ ]] || exit 2
ssh_safe() { ssh -T -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=10 "$host" "$@"; }
portable_mode() {
  local value
  value="$(stat -f %Lp "$1" 2>/dev/null || true)"
  if [[ "$value" =~ ^[0-7]{3,4}$ ]]; then printf '%s\n' "$value"; return; fi
  stat -c %a "$1"
}
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-evidence-remote.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
remote="$(ssh_safe "mktemp -d -p $base aura-evidence-remote.XXXXXX")"
[[ "$remote" == "$base"/aura-evidence-remote.* ]] || exit 1
cleanup() { ssh_safe "rm -rf -- $remote" || true; rm -rf -- "$fixture"; }
trap cleanup EXIT
project="$fixture/project"
runs="$project/.tmp/e2e/run/host-a/artifacts/runs"
mkdir -m 700 -p "$runs" "$project/target" "$fixture/shared" "$fixture/fakebin"
printf '[package]\nname="evidence-fixture"\nversion="0.1.0"\n' > "$project/Cargo.toml"
printf '/.tmp/\n/target/\n' > "$project/.gitignore"
git -C "$project" init -q
git -C "$project" add Cargo.toml .gitignore
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
make_run() {
  local id="$1" outcome="$2" pinned="$3" epoch="$4" path="$runs/$1"
  mkdir -m 700 "$path"
  printf 'protected %s\n' "$id" > "$path/evidence.log"
  chmod 600 "$path/evidence.log"
  jq -n --arg id "$id" --arg outcome "$outcome" --argjson pinned "$pinned" \
    --argjson epoch "$epoch" \
    '{schema_version:1,run_id:$id,state:"completed",outcome:$outcome,
      pinned:$pinned,finished_epoch:$epoch}' > "$path/.aura-retention.json"
  chmod 600 "$path/.aura-retention.json"
}
make_run failed-old failed false 1
make_run success-old success false 2
make_run success-new success false 3
make_run pinned-old failed true 0
cat > "$fixture/fakebin/ps" <<'PS'
#!/usr/bin/env bash
[[ "${FIXTURE_ACTIVE:-0}" == 1 ]] && printf '123 aura-harness\n'
exit 0
PS
chmod +x "$fixture/fakebin/ps"
export PATH="$fixture/fakebin:$PATH" AURA_COLD_CACHE_REMOTE_HOST="$host" \
  AURA_COLD_CACHE_REMOTE_DIR="$remote" AURA_BUILD_SHARED_DIR="$fixture/shared" \
  AURA_BUILD_MIN_FREE_GIB=0 AURA_COLD_CACHE_REMOTE_MIN_FREE_GIB=0
unset AURA_COLD_CACHE_STORE
cold() { bash "$repo_root/scripts/dev/cold-cache-store.sh" --root "$project" \
  --run-host host-a --run-id "$1" "$2"; }
refuse() {
  local expected="$1"; shift
  if "$@" > "$fixture/output" 2>&1; then
    echo "unexpected evidence operation success: $*" >&2; exit 1
  fi
  rg -q "$expected" "$fixture/output" || { cat "$fixture/output" >&2; exit 1; }
}
refuse 'not safely inactive' cold pinned-old --export
refuse 'newest completed success remains local' cold success-new --export
FIXTURE_ACTIVE=1 refuse 'harness consumer active' cold failed-old --export
mkdir -m 700 "$runs/.aura-retention.lock"
refuse 'retention owner lock busy' cold failed-old --export
rmdir "$runs/.aura-retention.lock"
ln -s "$project/Cargo.toml" "$runs/failed-old/linked"
refuse 'not safely inactive' cold failed-old --export
rm "$runs/failed-old/linked"
ln "$runs/failed-old/evidence.log" "$runs/failed-old/hardlinked"
refuse 'not safely inactive' cold failed-old --export
rm "$runs/failed-old/hardlinked"
exec 9< "$runs/failed-old/evidence.log"
refuse 'lane has open files' cold failed-old --export
exec 9<&-
AURA_COLD_CACHE_REMOTE_MIN_FREE_GIB=99999 refuse 'breach capacity floor' cold failed-old --export
[[ -f "$runs/failed-old/evidence.log" ]]
cold failed-old --export > "$fixture/output"
[[ ! -e "$runs/failed-old" ]]
revision="$(git -C "$project" rev-parse HEAD)"
key="$(printf '%s' "$project" | sha256sum | awk '{print $1}')"
lane=evidence-h6-host-a-r10-failed-old
bucket="$fixture/shared/cold-remote-receipts/aura-cold-cache/$key/$revision/$lane"
ready="$remote/aura-cold-cache/$key/$revision/$lane/ready"
[[ -f "$bucket/location.json" && -f "$bucket/location.sig" ]]
source_relative=".tmp/e2e/run/host-a/artifacts/runs/failed-old"
jq -e --arg relative "$source_relative" \
  '.source_relative==$relative and (.metadata_sha256|test("^[0-9a-f]{64}$"))' \
  "$bucket/location.json" >/dev/null
ssh_safe "cat $ready/manifest.json" > "$fixture/manifest-original.json"
jq -e --arg relative "$source_relative" \
  '.source_relative==$relative and (.metadata_sha256|test("^[0-9a-f]{64}$"))' \
  "$fixture/manifest-original.json" >/dev/null
ssh_safe "test \"\$(stat -c %a $ready)\" = 700; test \"\$(stat -c %a $ready/members.tsv)\" = 600"
ssh_safe "cat $ready/members.tsv" > "$fixture/members-original.tsv"
mkdir -m 700 "$runs/failed-old"
printf 'occupied\n' > "$runs/failed-old/evidence.log"
refuse 'already exists' cold failed-old --restore
rm -rf "$runs/failed-old"
ssh_safe "printf tamper >> $ready/members.tsv"
refuse 'metadata manifest mismatch' cold failed-old --restore
[[ ! -e "$runs/failed-old" ]]
ssh_safe "cat > $ready/members.tsv" < "$fixture/members-original.tsv"
cp "$bucket/location.json" "$fixture/location-original.json"
cp "$bucket/location.sig" "$fixture/location-original.sig"
jq '.source_relative=".tmp/e2e/run/host-a/artifacts/runs/foreign"' \
  "$fixture/location-original.json" > "$bucket/location.json"
openssl dgst -sha256 -sign "$fixture/shared/cold-keys/$key.pem" \
  -out "$bucket/location.sig" "$bucket/location.json"
refuse 'location identity mismatch' cold failed-old --restore
[[ ! -e "$runs/failed-old" ]]
cp "$fixture/location-original.json" "$bucket/location.json"
cp "$fixture/location-original.sig" "$bucket/location.sig"
cold failed-old --restore > "$fixture/output"
[[ "$(cat "$runs/failed-old/evidence.log")" == 'protected failed-old' ]]
[[ "$(portable_mode "$runs/failed-old")" == 700 &&
   "$(portable_mode "$runs/failed-old/evidence.log")" == 600 ]]
refuse 'existing location receipt refuses overwrite' cold failed-old --export
make_run failed-open failed false 4
cat > "$fixture/fakebin/ssh" <<'OPEN_AFTER_UPLOAD'
#!/usr/bin/env bash
if [[ "$*" == *'/evidence-h6-host-a-r11-failed-open/ready/archive.tar'* &&
      "$*" == *'cat '* ]]; then
  bash -c 'exec 9< "$AURA_TEST_OPEN_FILE"; sleep 60' >/dev/null 2>&1 &
  printf '%s\n' "$!" > "$AURA_TEST_OPEN_PID"
fi
exec /usr/bin/ssh "$@"
OPEN_AFTER_UPLOAD
chmod +x "$fixture/fakebin/ssh"
AURA_TEST_OPEN_FILE="$runs/failed-open/evidence.log" \
  AURA_TEST_OPEN_PID="$fixture/open-pid" \
  refuse 'evidence became open before guarded prune' cold failed-open --export
[[ -f "$runs/failed-open/evidence.log" ]]
kill "$(cat "$fixture/open-pid")"
rm "$fixture/fakebin/ssh"
make_run failed-partial failed false 5
cat > "$fixture/fakebin/ssh" <<'PARTIAL_UPLOAD'
#!/usr/bin/env bash
[[ "$*" != *'cat > '*'location.sig'* ]] || exit 23
exec /usr/bin/ssh "$@"
PARTIAL_UPLOAD
chmod +x "$fixture/fakebin/ssh"
refuse 'remote location upload stopped before ready' cold failed-partial --export
[[ -f "$runs/failed-partial/evidence.log" ]]
partial_lane=evidence-h6-host-a-r14-failed-partial
ssh_safe "test -d $remote/aura-cold-cache/$key/$revision/$partial_lane/.pending; test ! -e $remote/aura-cold-cache/$key/$revision/$partial_lane/ready"
rm "$fixture/fakebin/ssh"
printf '# revised source\n' >> "$project/Cargo.toml"
git -C "$project" add Cargo.toml
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm revised
rm -rf "$runs/failed-old"
refuse 'protected location receipt absent' cold failed-old --restore
echo 'evidence remote: intended guard and signed archive cases reached'
