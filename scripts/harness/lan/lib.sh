# Source me: shell helpers for driving a multi-host harness run through each
# host's drv.sh. Instances whose id starts with $AURA_E2E_REMOTE_PREFIX are
# sent to $AURA_E2E_REMOTE over ssh; all others go to the local driver.
# Settings: see env.sh. Requires jq.
# shellcheck source=env.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
AURA_E2E_DRV="$AURA_E2E_ROOT/scripts/harness/lan/drv.sh"

req() { # req <inst> <method> [extra-json-fields] [timeout]
  local inst=$1 m=$2 extra=${3:-} to=${4:-60}
  local body="{\"method\":\"$m\",\"params\":{\"instance_id\":\"$inst\"${extra:+,$extra}}}"
  if [ -n "$AURA_E2E_REMOTE" ] && [[ "$inst" == "$AURA_E2E_REMOTE_PREFIX"* ]]; then
    ssh -o BatchMode=yes "$AURA_E2E_REMOTE" \
      "cd $AURA_E2E_REMOTE_ROOT && scripts/harness/lan/drv.sh req '$body' $to"
  else
    "$AURA_E2E_DRV" req "$body" "$to"
  fi
}
key()  { req "$1" send_key "\"key\":\"$2\"" >/dev/null; sleep "${3:-0.5}"; }
keys() { req "$1" send_keys "\"keys\":\"$2\"" "" 120 >/dev/null; sleep "${3:-0.4}"; }
st()   { req "$1" ui_state | jq -c ".payload|${2:-.}"; }
scr()  { req "$1" screen | jq -r .payload.diagnostic_authoritative_screen | sed -n "${2:-1,200p}"; }
chans() { req "$1" diagnostic_list_channels | jq -c '[.payload.diagnostic_channels[]|(if .selected then "*" else "" end)+.name]'; }
contacts() { st "$1" '[.lists[]|select(.id=="contacts")|.items[].id]'; }
lastcode() { st "$1" '[.runtime_events[]|select(.fact.kind=="'"${2:-invitation_code_ready}"'")][-1].fact.code' | tr -d '"'; }
ops()  { st "$1" '[.operations[]|"\(.id)=\(.state)"]'; }
toasts() { st "$1" '[.toasts[]|.message[0:200]]'; }
sel() { st "$1" ".selections[]|select(.list==\"$2\")|.item_id" | tr -d '"'; }
restart_ok() { # restart <inst> and report whether the account came back
  req "$1" restart "" 300 >/dev/null; sleep 4
  local a; a=$(req "$1" get_authority_id | jq -r '.payload.authority_id // ("ERR " + (.message[0:160]))'); echo "$1 after restart: $a"
}
onboard_tui() { keys "$1" "$2"; key "$1" enter 4; req "$1" get_authority_id | jq -r .payload.authority_id; }
normal() { # leave insert mode, close modals, dismiss toasts and clear the chat draft on a TUI
  for _ in 1 2 3 4 5; do key "$1" esc 0.3; done
  req "$1" send_key '"key":"backspace","repeat":200' >/dev/null 2>&1; for _ in 1 2; do key "$1" esc 0.3; done
}
link() { # link <inviter> <accepter>: contact invite from inviter, imported by accepter
  normal "$1"; keys "$1" 3n 1; key "$1" enter 4; local c; c=$(lastcode "$1"); key "$1" esc
  normal "$2"; keys "$2" 3a 1; keys "$2" "$c" 1; key "$2" enter 6; echo "link $1->$2 code=${#c}"
}
pick_contact() { # pick_contact <inst> <authority-id>: move the contacts selection to the id
  normal "$1"; keys "$1" 3 1
  for _ in 1 2 3 4 5 6; do [ "$(sel "$1" contacts)" = "$2" ] && return 0; keys "$1" j 0.6; done; return 1
}
act_notif() { # act_notif <inst> <id-prefix> <key>: select the first matching notification, press key
  normal "$1"; keys "$1" 4 1
  for _ in 1 2 3 4 5 6 7 8; do case "$(sel "$1" notifications)" in "$2"*) keys "$1" "$3" 3; return 0;; esac; keys "$1" j 0.6; done
  for _ in 1 2 3 4 5 6 7 8; do case "$(sel "$1" notifications)" in "$2"*) keys "$1" "$3" 3; return 0;; esac; keys "$1" k 0.6; done; return 1
}
rlog() { # rlog <inst>: newest runtime.log for the instance (run on the owning host)
  ls -t "$AURA_E2E_ROOT"/.tmp/harness/transient/*/"$1"/runtime.log 2>/dev/null | head -1
}
