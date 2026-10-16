#!/usr/bin/env bash
# Exact delivery, missed-wakeup recovery, refusal, redaction and interruption.
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-receipt-events.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT
project="$fixture/project"
shared="$fixture/shared"
mkdir -p "$project/target" "$shared" "$fixture/bin"
chmod 700 "$shared"
git -C "$project" init -q
printf '[package]\nname="receipt-event-fixture"\nversion="0.0.0"\n' > "$project/Cargo.toml"
git -C "$project" add Cargo.toml
git -C "$project" -c user.name=fixture -c user.email=fixture@example.invalid commit -qm fixture
export AURA_BUILD_SHARED_DIR="$shared" AURA_BUILD_WAIT_SECONDS=0
export AURA_BUILD_MIN_FREE_GIB=15 AURA_BUILD_RESERVE_GIB=4
export FIXTURE_FREE="$fixture/free"
printf '41943040\n' > "$FIXTURE_FREE"
cat > "$fixture/bin/df" <<'EOF'
#!/usr/bin/env bash
printf 'Filesystem 1024-blocks Used Available Capacity Mounted\n'
printf 'fake 99999999 0 %s 0%% /\n' "$(cat "$FIXTURE_FREE")"
EOF
cat > "$fixture/bin/cargo" <<'EOF'
#!/usr/bin/env bash
[[ "${1:-}" == sweep ]] || exit 2
printf 'sweep fixture preview\n'
EOF
chmod +x "$fixture/bin/df" "$fixture/bin/cargo"
export PATH="$fixture/bin:$PATH"
events="$repo/scripts/dev/receipt-events.sh"
budget="$repo/scripts/dev/build-budget.sh"

fifo="$(bash "$events" --create)"
timeout 10s bash "$events" --next "$fifo" > "$fixture/wakeup" &
listener=$!
AURA_BUILD_EVENT_FIFO="$fifo" bash "$budget" --root "$project" --lane event-fixture \
  --no-prune -- sh -c 'exit 0' > "$fixture/success" 2>&1
wait "$listener"
receipt="$(sed -n 's/^Build receipt: //p' "$fixture/success")"
jq -e --arg receipt "$receipt" --arg source "$(jq -r .source.fingerprint "$receipt")" \
  '.kind=="admission" and .sequence==1 and .receipt==$receipt and .source_fingerprint==$source' \
  "$fixture/wakeup" >/dev/null
[[ "$(jq -r '.event_delivery[0].status' "$receipt")" == sent ]]
bash "$events" --recover "$receipt" 1 > "$fixture/recovered"
jq -e -s 'length==1 and .[0].kind=="completion" and .[0].sequence==2 and
  .[0].exit_status==0' "$fixture/recovered" >/dev/null
chmod 644 "$receipt"
bash "$events" --recover "$receipt" 0 > "$fixture/public-recovery" 2>&1 && exit 1
chmod 600 "$receipt"
# A wakeup is only a pointer. Reject a forged source, and never forward extra
# sender-controlled fields even when the pointer names a genuine event.
timeout 10s bash "$events" --next "$fifo" > "$fixture/forged" &
listener=$!
jq -cn --arg receipt "$receipt" '{receipt:$receipt,sequence:1,kind:"admission",
  source_fingerprint:"foreign"}' > "$fifo"
wait "$listener" && exit 1
[[ ! -s "$fixture/forged" ]]
# A named FIFO masquerading as a receipt must be rejected before jq opens it.
fake_run="$project/artifacts/disk-budget/receipts/run.FAKE12"
mkdir -m 700 "$fake_run"
mkfifo -m 600 "$fake_run/receipt.json"
timeout 10s bash "$events" --next "$fifo" > "$fixture/nonregular" &
listener=$!
jq -cn --arg receipt "$fake_run/receipt.json" \
  '{receipt:$receipt,sequence:1,kind:"admission",source_fingerprint:"foreign"}' > "$fifo"
wait "$listener" && exit 1
[[ ! -s "$fixture/nonregular" ]]
timeout 10s bash "$events" --next "$fifo" > "$fixture/extra" &
listener=$!
jq -cn --arg receipt "$receipt" --arg source "$(jq -r .source.fingerprint "$receipt")" \
  '{receipt:$receipt,sequence:1,kind:"admission",source_fingerprint:$source,
    secret:"private-event-value"}' > "$fifo"
wait "$listener"
jq -e '.kind=="admission" and (.secret==null)' "$fixture/extra" >/dev/null
! rg -q 'private-event-value' "$fixture/extra"
bash "$events" --close "$fifo"
mkdir -p "$shared/event-subscribers/listener.fake/nested"
mkfifo "$shared/event-subscribers/listener.fake/nested/event.fifo"
bash "$events" --next "$shared/event-subscribers/listener.fake/nested/event.fifo" \
  > "$fixture/nested" 2>&1 && exit 1

# Two concurrent dependency writers serialize sequence allocation. Repeating
# one dependency does not duplicate its event or wakeup.
export AURA_BUILD_RECEIPT="$receipt"
for dependency in first second; do
  AURA_RECEIPT_EVENT_KIND=dependency bash -c '
    source "$1/scripts/dev/build-receipt.sh"
    aura_receipt_update --arg dependency "$2" \
      ".metadata.policy_compile_receipt=\$dependency"' _ "$repo" "$dependency" &
done
wait
jq -e '([.events[].sequence]|sort)==[1,2,3,4] and
  ([.events[]|select(.kind=="dependency")]|length)==2' "$receipt" >/dev/null
AURA_RECEIPT_EVENT_KIND=dependency bash -c '
  source "$1/scripts/dev/build-receipt.sh"
  aura_receipt_update --arg dependency second ".metadata.policy_compile_receipt=\$dependency"' _ "$repo"
jq -e '(.events|length)==4' "$receipt" >/dev/null
# Concurrent first-failure publishers cannot replace the first retained cause.
for cause in first-cause second-cause; do
  AURA_RECEIPT_EVENT_KIND=first-failure bash -c '
    source "$1/scripts/dev/build-receipt.sh"
    aura_receipt_update --arg failure "$2" ".failure_excerpt=\$failure"' _ "$repo" "$cause" &
done
wait
jq -e '([.events[]|select(.kind=="first-failure")]|length)==1 and
  .failure_excerpt==([.events[]|select(.kind=="first-failure")][0].failure)' "$receipt" >/dev/null
unset AURA_BUILD_RECEIPT

# A linked or publicly readable sink is rejected without affecting the build.
unsafe="$shared/event-subscribers/unsafe.fifo"
ln -s "$fixture/missing" "$unsafe"
AURA_BUILD_EVENT_FIFO="$unsafe" bash "$budget" --root "$project" --lane event-fixture \
  --no-prune -- sh -c 'exit 0' > "$fixture/invalid-sink" 2>&1
receipt="$(sed -n 's/^Build receipt: //p' "$fixture/invalid-sink")"
jq -e '.final_exit_status==0 and ([.event_delivery[].status]|all(.=="missed"))' "$receipt" >/dev/null
rm "$unsafe"
# Both Nix GNU stat and host BSD stat must yield numeric owner/mode witnesses.
for stat_path in "$(command -v stat)" /usr/bin/stat; do
  [[ -x "$stat_path" ]] || continue
  fixture_stat_dir="$(dirname "$stat_path")"
  PATH="$fixture_stat_dir:$PATH" bash -c '
    source "$1/scripts/dev/build-receipt.sh"
    [[ "$(aura_receipt_stat_owner "$2")" == "$(id -u)" &&
      "$(aura_receipt_stat_mode "$2")" == 700 ]]' _ "$repo" "$shared"
done

# Closed or invalid sinks are advisory. Secret output remains only in the
# protected command log, while the first cause and completion are durable.
AURA_BUILD_EVENT_FIFO="$fifo" bash "$budget" --root "$project" --lane event-fixture \
  --no-prune -- sh -c 'printf "private-event-value\n"; exit 37' > "$fixture/failure" 2>&1 && exit 1
receipt="$(sed -n 's/^Build receipt: //p' "$fixture/failure")"
jq -e '.final_exit_status==37 and [.events[].kind]==["admission","first-failure","completion"] and
  ([.event_delivery[].status]|all(.=="missed"))' "$receipt" >/dev/null
! rg -q 'private-event-value' "$receipt" "$(dirname "$receipt")/completion.json"
rg -q 'private-event-value' "$(jq -r .artifacts.command_log "$receipt")"

# Admission refusal never starts a command and still yields terminal evidence.
printf '16252928\n' > "$FIXTURE_FREE"
bash "$budget" --root "$project" --lane event-fixture --no-prune -- \
  sh -c 'exit 89' > "$fixture/refusal" 2>&1 && exit 1
receipt="$(sed -n 's/^Build receipt: //p' "$fixture/refusal")"
jq -e '.admission.admitted==false and .command_exit_status==null and
  .final_exit_status==1 and [.events[].kind]==["admission","first-failure","completion"] and
  .events[1].failure=="build admission refused; details in receipt"' "$receipt" >/dev/null

# A terminated child retains one terminal event and can be recovered after a
# reader restart. The build owner handles its own child shutdown.
printf '41943040\n' > "$FIXTURE_FREE"
timeout -s TERM 3s bash "$budget" --root "$project" --lane event-fixture \
  --no-prune -- sh -c 'sleep 30' > "$fixture/interrupted" 2>&1 && exit 1
receipt="$(sed -n 's/^Build receipt: //p' "$fixture/interrupted")"
jq -e '.final_exit_status!=null and ([.events[]|select(.kind=="completion")]|length)==1' \
  "$receipt" >/dev/null
bash "$events" --recover "$receipt" 0 > "$fixture/interrupted-recovered"
jq -e -s 'length>=2 and .[-1].kind=="completion"' "$fixture/interrupted-recovered" >/dev/null
printf 'receipt event delivery/recovery/refusal/redaction/interruption passed\n'
