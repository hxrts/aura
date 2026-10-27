#!/usr/bin/env bash
# Real tiny archive/sign/readback/restore, controlled SSH identity transport.
# This does not claim remote hardware/ext4/capacity or Rust producer execution.
set -euo pipefail
repo_root=$(cd "$(dirname "$0")/../.." && pwd -P)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-native-retention.XXXXXX")
fixture=$(cd "$fixture" && pwd -P)
finish_fixture() {
  local status=$?
  if (( status == 0 )); then rm -rf "$fixture";
  else printf 'failed fixture retained: %s\n' "$fixture" >&2; fi
  exit "$status"
}
trap finish_fixture EXIT
umask 077
root=$fixture/project
remote=$fixture/remote
mkdir -p "$root/target" "$root/artifacts/disk-budget/receipts" "$remote" "$fixture/shared" "$fixture/fakebin"
chmod 700 "$remote" "$fixture/shared"
printf '/target/\n/artifacts/\n/.tmp/\n' > "$root/.gitignore"
printf 'fixture source\n' > "$root/source"
git -C "$root" init -q
git -C "$root" add .gitignore source
git -C "$root" -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm fixture
revision=$(git -C "$root" rev-parse HEAD)
source "$repo_root/scripts/dev/build-receipt.sh"
fp=$(aura_receipt_source_fingerprint "$root")
printf '#!/bin/sh\nexec "$FIXTURE_SHELL" "$@"\n' > "$fixture/fakebin/bash"
cat > "$fixture/fakebin/ssh" <<'SSH'
#!/usr/bin/env bash
set -euo pipefail
while [[ $# -gt 0 && $1 != fixture@host ]]; do shift; done
[[ ${1:-} == fixture@host ]] || exit 47
shift
if [[ $* == 'bash -s -- '* ]]; then
 script=$(mktemp "$FIXTURE_TMP/ssh-script.XXXXXX")
 cat > "$script"
 if grep -q 'findmnt -n -o FSTYPE' "$script"; then
  if [[ -n ${FIXTURE_MUTATE_RECEIPT:-} ]]; then printf ' ' >> "$FIXTURE_MUTATE_RECEIPT"; fi
  printf '11111111111111111111111111111111 /dev/fixture 99999999\n'
  rm "$script"; exit 0
 fi
 if [[ ${FIXTURE_FAIL_UPLOAD:-0} == 1 ]] && grep -q 'mv -T -- "$pending" "$ready"' "$script"; then
  rm "$script"; printf 'fixture original publication failure\n' >&2; exit 47
 fi
 "$FIXTURE_SHELL" "$script" ${*#bash -s -- }
 result=$?; rm "$script"; exit "$result"
fi
exec "$FIXTURE_SHELL" -c "$*"
SSH
for command in lsof ps; do printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/fakebin/$command"; done
cat > "$fixture/fakebin/ps" <<'PS'
#!/usr/bin/env bash
if [[ ${1:-} == -p ]]; then exec /bin/ps "$@"; fi
exit 0
PS
cat > "$fixture/fakebin/lsof" <<'LSOF'
#!/usr/bin/env bash
[[ -z ${FIXTURE_OPEN_PATH:-} ]] || printf 'controlled open %s\n' "$FIXTURE_OPEN_PATH"
exit 0
LSOF
chmod +x "$fixture/fakebin/"*
export PATH="$fixture/fakebin:$PATH" AURA_COLD_CACHE_REMOTE_HOST=fixture@host AURA_COLD_CACHE_REMOTE_DIR="$remote"
export AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=0 AURA_COLD_CACHE_REMOTE_MIN_FREE_GIB=0
export FIXTURE_TMP="$fixture" FIXTURE_SHELL="$BASH"
unset AURA_COLD_CACHE_STORE
# These expectations are captured once by the original synthetic producer/control
# before retention; cold() never refreshes an expected digest from mutable data.
binding_ids=() binding_digests=()
mkdir "$fixture/bindings"
capture_fixture_binding() {
 local id=$1 run="$root/artifacts/disk-budget/receipts/$1" r c b i value index
 r=$(b3sum "$run/receipt.json"); r=${r%% *}
 c=$(b3sum "$run/completion.json"); c=${c%% *}
 b=$(b3sum "$run/test-binary.abc123/probe"); b=${b%% *}
 i=$(b3sum "$run/inventory.abc123"); i=${i%% *}
 jq -cS --arg receipt "$run/receipt.json" --arg r "$r" --arg c "$c" --arg binary "$b" --arg inventory "$i" \
  '{receipt:$receipt,source:.source,config:(.metadata|{packages,features,test_target,test_filter,test_inventory,expected_count}),
    receipt_blake3:$r,completion_blake3:$c,binary_blake3:$binary,inventory_blake3:$inventory,
    protected_executable:.artifacts.protected_test_executable}' "$run/receipt.json" > "$fixture/bindings/$id.json"
 chmod 600 "$fixture/bindings/$id.json"
 value=$(b3sum "$fixture/bindings/$id.json"); value=${value%% *}
 for index in "${!binding_ids[@]}"; do
  if [[ ${binding_ids[$index]} == "$id" ]]; then binding_digests[$index]=$value; return 0; fi
 done
 binding_ids[${#binding_ids[@]}]=$id
 binding_digests[${#binding_digests[@]}]=$value
}
make_run() {
 local id=$1 status=$2 run producer protected digest
 run="$root/artifacts/disk-budget/receipts/$id"
 mkdir -m 700 "$run" "$run/test-binary.abc123"
 producer="$root/target/debug/deps/probe"
 protected="$run/test-binary.abc123/probe"
 printf '#!/bin/sh\nexit %s\n' "$status" > "$protected"
 chmod 500 "$protected"
 printf 'fixture_exact\n' > "$run/inventory.abc123"
 chmod 600 "$run/inventory.abc123"
 digest=$(b3sum "$protected"); digest=${digest%% *}
 jq -n --arg root "$root" --arg rev "$revision" --arg fp "$fp" --arg run "$run" \
  --arg protected "$protected" --arg producer "$producer" --arg digest "$digest" --argjson status "$status" \
  '{schema:2,root:$root,command_argv:["bash",($root+"/scripts/dev/run-exact-fixtures.sh"),"--inside","--replay-first-failure"],source:{revision:$rev,fingerprint:$fp},notification:($run+"/completion.json"),
    admission:{admitted:true},current_phase:"finished",active_phase:null,finished_utc:"2026-10-26T00:00:00Z",
    command_exit_status:$status,final_exit_status:$status,metadata:{replay_first_failure:true,expected_count:1,
      packages:"fixture",test_target:"lib",test_inventory:"fixture_exact",producer_binary_blake3:($digest+"  "+$producer)},
    artifacts:{test_executable:$producer,expected_inventory:($run+"/inventory.abc123"),protected_test_executable:$protected,
      protected_test_executable_blake3:($digest+"  "+$protected)},
    phases:[{phase:"compile",exit_status:0,command_exit_status:0}],
    events:[{kind:"completion",source_fingerprint:$fp,exit_status:$status}],
    wrapper:{pid:999999,start_identity:"absent fixture wrapper"},child:{pid:999998,start_identity:"absent fixture child"}}' > "$run/receipt.json"
 jq -n --arg run "$run" --argjson status "$status" '{receipt:($run+"/receipt.json"),exit_status:$status,finished_utc:"2026-10-26T00:00:00Z"}' > "$run/completion.json"
 chmod 600 "$run/receipt.json" "$run/completion.json"
 capture_fixture_binding "$id"
}
cold() {
 local id=$1 action=$2 index expected=''
 for index in "${!binding_ids[@]}"; do
  [[ ${binding_ids[$index]} != "$id" ]] || expected=${binding_digests[$index]}
 done
 [[ -n $expected ]] || return 2
 "$BASH" "$repo_root/scripts/dev/cold-cache-store.sh" --root "$root" --native-run "$id" \
  --native-binding "$fixture/bindings/$id.json" --native-binding-blake3 "$expected" "$action"
}
refuse() {
 local expected_status=$1 cause=$2 actual=0; shift 2
 "$@" > "$fixture/output" 2>&1 || actual=$?
 [[ $actual == "$expected_status" ]] || { cat "$fixture/output"; echo "wrong native status $actual expected $expected_status" >&2; exit 1; }
 grep -F "$cause" "$fixture/output" >/dev/null || { cat "$fixture/output"; exit 1; }
}
mutate() { jq "$2" "$root/artifacts/disk-budget/receipts/$1/receipt.json" > "$fixture/changed"; cp "$fixture/changed" "$root/artifacts/disk-budget/receipts/$1/receipt.json"; }
! kill -0 999999 2>/dev/null && ! kill -0 999998 2>/dev/null
make_run run.positive 0
run="$root/artifacts/disk-budget/receipts/run.positive"
receipt_digest=$(b3sum "$run/receipt.json"); completion_digest=$(b3sum "$run/completion.json")
# Source/static receipt controls each change just one known valid fixture field.
cp "$run/receipt.json" "$fixture/original-receipt"
mutate run.positive '.current_phase="execution"|.active_phase="execution"'
refuse 78 'native expected identity differs' cold run.positive --export
cp "$fixture/original-receipt" "$run/receipt.json"
mutate run.positive '.command_exit_status=101'
refuse 78 'native expected identity differs' cold run.positive --export
cp "$fixture/original-receipt" "$run/receipt.json"
mutate run.positive '.source.fingerprint="foreign"'
refuse 78 'native expected identity differs' cold run.positive --export
cp "$fixture/original-receipt" "$run/receipt.json"
mutate run.positive '.phases[0].exit_status=101'
refuse 78 'native expected identity differs' cold run.positive --export
cp "$fixture/original-receipt" "$run/receipt.json"
mutate run.positive '.command_argv[1]="/foreign/scripts/dev/run-exact-fixtures.sh"'
refuse 78 'native expected identity differs' cold run.positive --export
cp "$fixture/original-receipt" "$run/receipt.json"
mutate run.positive '.admission.admitted=false'
refuse 78 'native expected identity differs' cold run.positive --export
cp "$fixture/original-receipt" "$run/receipt.json"
live_start=$(/bin/ps -p "$$" -o lstart=)
jq --argjson pid "$$" --arg start "$live_start" '.wrapper={pid:$pid,start_identity:$start}' "$run/receipt.json" > "$fixture/changed"
cp "$fixture/changed" "$run/receipt.json"
refuse 78 'native expected identity differs' cold run.positive --export
cp "$fixture/original-receipt" "$run/receipt.json"
FIXTURE_OPEN_PATH="$run/test-binary.abc123/probe" refuse 1 'lane has open files' cold run.positive --export
cp "$run/completion.json" "$fixture/original-completion"
printf '{}\n' > "$run/completion.json"
refuse 78 'native expected identity differs' cold run.positive --export
cp "$fixture/original-completion" "$run/completion.json"
cp -p "$run/test-binary.abc123/probe" "$fixture/protected-original"
rm "$run/test-binary.abc123/probe"
ln -s "$fixture/protected-original" "$run/test-binary.abc123/probe"
refuse 1 'evidence source not safely inactive' cold run.positive --export
rm "$run/test-binary.abc123/probe"
cp -p "$fixture/protected-original" "$run/test-binary.abc123/probe"
printf 'other_exact\n' > "$run/inventory.abc123"
refuse 1 'evidence source not safely inactive' cold run.positive --export
printf 'fixture_exact\n' > "$run/inventory.abc123"
chmod 700 "$run/test-binary.abc123/probe"
refuse 1 'evidence source not safely inactive' cold run.positive --export
chmod 500 "$run/test-binary.abc123/probe"
ln "$run/test-binary.abc123/probe" "$fixture/alias"
refuse 1 'evidence source not safely inactive' cold run.positive --export
rm "$fixture/alias"
chmod 700 "$run/test-binary.abc123/probe"
printf 'tamper' >> "$run/test-binary.abc123/probe"
chmod 500 "$run/test-binary.abc123/probe"
refuse 78 'native expected artifact differs' cold run.positive --export
chmod 700 "$run/test-binary.abc123/probe"
printf '#!/bin/sh\nexit 0\n' > "$run/test-binary.abc123/probe"; chmod 500 "$run/test-binary.abc123/probe"
cold run.positive --export > "$fixture/export.log" 2>&1
[[ ! -e "$run" ]]
# Refuse source/config drift and corrupt archive without publishing partial restore.
printf 'different source\n' > "$root/source"
refuse 1 'original native source differs' cold run.positive --restore
[[ ! -e "$run" ]]
printf 'fixture source\n' > "$root/source"
archive=$(find "$remote" -path '*/completed-native-run.positive/ready/archive.tar' -print)
[[ -f "$archive" ]]
cp -p "$archive" "$fixture/original-archive"
printf 'corrupt' >> "$archive"
refuse 1 'remote archive hash or size mismatch' cold run.positive --restore
[[ ! -e "$run" ]]
cp -p "$fixture/original-archive" "$archive"
cold run.positive --restore > "$fixture/restore.log" 2>&1
[[ $(b3sum "$run/receipt.json") == "$receipt_digest" && $(b3sum "$run/completion.json") == "$completion_digest" ]]
"$run/test-binary.abc123/probe"
refuse 1 'restore lane already exists' cold run.positive --restore
# Failed actual test status survives original receipt/archive/restore; not a pass.
make_run run.failed 101
failed="$root/artifacts/disk-budget/receipts/run.failed"
failed_receipt=$(b3sum "$failed/receipt.json")
cold run.failed --export > "$fixture/failed-export.log" 2>&1
cold run.failed --restore > "$fixture/failed-restore.log" 2>&1
[[ $(b3sum "$failed/receipt.json") == "$failed_receipt" ]]
actual=0; "$failed/test-binary.abc123/probe" || actual=$?
[[ $actual == 101 ]]
# Publication failure must preserve the complete original and native47.
make_run run.partial 0
FIXTURE_FAIL_UPLOAD=1 refuse 47 'fixture original publication failure' cold run.partial --export
[[ -f "$root/artifacts/disk-budget/receipts/run.partial/receipt.json" && -x "$root/artifacts/disk-budget/receipts/run.partial/test-binary.abc123/probe" ]]
[[ ! -e $(find "$remote" -path '*/completed-native-run.partial/ready' -print) ]]
make_run run.raced 0
raced="$root/artifacts/disk-budget/receipts/run.raced"
FIXTURE_MUTATE_RECEIPT="$raced/receipt.json" refuse 78 'native artifact changed before signing' cold run.raced --export
[[ -f "$raced/receipt.json" && -x "$raced/test-binary.abc123/probe" ]]
[[ -z $(find "$remote" -path '*/completed-native-run.raced/ready' -print) ]]
make_run run.swapped 0
swapped="$root/artifacts/disk-budget/receipts/run.swapped"
cp "$fixture/bindings/run.swapped.json" "$fixture/held-original-binding"
chmod 700 "$swapped/test-binary.abc123/probe"
printf '#!/bin/sh\nexit 23\n' > "$swapped/test-binary.abc123/probe"
chmod 500 "$swapped/test-binary.abc123/probe"
replacement=$(b3sum "$swapped/test-binary.abc123/probe"); replacement=${replacement%% *}
jq --arg b "$replacement" '.command_exit_status=23|.final_exit_status=23|.events[0].exit_status=23|
  .metadata.producer_binary_blake3=($b+"  "+.artifacts.test_executable)|
  .artifacts.protected_test_executable_blake3=($b+"  "+.artifacts.protected_test_executable)' "$swapped/receipt.json" > "$fixture/changed"
cp "$fixture/changed" "$swapped/receipt.json"
jq '.exit_status=23' "$swapped/completion.json" > "$fixture/changed"; cp "$fixture/changed" "$swapped/completion.json"
refuse 78 'native expected identity differs' cold run.swapped --export
[[ -f "$swapped/receipt.json" && -x "$swapped/test-binary.abc123/probe" ]]
cmp "$fixture/bindings/run.swapped.json" "$fixture/held-original-binding"
# Replacing the external expectation file cannot replace its held literal digest.
printf ' ' >> "$fixture/bindings/run.swapped.json"
refuse 78 'native expectation file differs' cold run.swapped --export
# A caller-held contradictory record still cannot certify terminal success.
make_run run.contradict 0
mutate run.contradict '.command_exit_status=101'
capture_fixture_binding run.contradict
refuse 1 'evidence source not safely inactive' cold run.contradict --export
make_run run.live 0
live="$root/artifacts/disk-budget/receipts/run.live"
live_start=$(/bin/ps -p "$$" -o lstart=)
jq --argjson pid "$$" --arg start "$live_start" '.wrapper={pid:$pid,start_identity:$start}' "$live/receipt.json" > "$fixture/changed"
cp "$fixture/changed" "$live/receipt.json"
capture_fixture_binding run.live
refuse 1 'evidence source not safely inactive' cold run.live --export
# Native executable permission is not an exemption for default LAN payloads.
lan="$root/.tmp/e2e/run/fixture/artifacts/runs/failed-old"
mkdir -m 700 -p "$lan"
printf 'legacy diagnostic\n' > "$lan/evidence.log"
printf '{"schema_version":1,"run_id":"failed-old","state":"completed","outcome":"failed","pinned":false,"finished_epoch":1}\n' > "$lan/.aura-retention.json"
chmod 600 "$lan/evidence.log" "$lan/.aura-retention.json"
chmod 500 "$lan/evidence.log"
refuse 1 'evidence source not safely inactive' "$BASH" "$repo_root/scripts/dev/cold-cache-store.sh" --root "$root" --run-host fixture --run-id failed-old --export
chmod 600 "$lan/evidence.log"
"$BASH" "$repo_root/scripts/dev/cold-cache-store.sh" --root "$root" --run-host fixture --run-id failed-old --export > "$fixture/lan-export.log" 2>&1
"$BASH" "$repo_root/scripts/dev/cold-cache-store.sh" --root "$root" --run-host fixture --run-id failed-old --restore > "$fixture/lan-restore.log" 2>&1
[[ $(cat "$lan/evidence.log") == 'legacy diagnostic' ]]
printf 'completed native applicability/positive restore/original101/tamper/partial-publication controls passed\n'
