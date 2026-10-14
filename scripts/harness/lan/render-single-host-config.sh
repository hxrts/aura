#!/usr/bin/env bash
# Sourced by the single-host diagnostic and its fixture. Keep LAN templates intact.
render_single_host_config() { # template, output, local|peer, broker auth
  local template=$1 output=$2 role=$3 auth=$4 expected line id mode invitation
  local selected=0 in_instance=0
  local -a block=()
  [[ "$role" == local || "$role" == peer ]] || return 2
  [[ "$auth" =~ ^[0-9a-f]{64}$ && -f "$template" && ! -e "$output" && ! -L "$output" ]] || return 2
  if [[ "$role" == local ]]; then expected=alex-tui; else expected=barbara-tui; fi
  : > "$output" || return
  chmod 600 "$output" || return
  _single_host_emit() {
    local entry rendered
    id='' mode=''
    for entry in "${block[@]}"; do
      if [[ "$entry" =~ ^id[[:space:]]*=[[:space:]]*\"([^\"]+)\"$ ]]; then id=${BASH_REMATCH[1]}; fi
      if [[ "$entry" =~ ^mode[[:space:]]*=[[:space:]]*\"([^\"]+)\"$ ]]; then mode=${BASH_REMATCH[1]}; fi
    done
    [[ "$id" == "$expected" ]] || return 0
    [[ "$mode" == local ]] || return 1
    selected=$((selected + 1))
    invitation=$(openssl rand -hex 32) || return
    [[ "$invitation" =~ ^[0-9a-f]{64}$ ]] || return 1
    for entry in "${block[@]}"; do
      rendered=${entry//__HOST_ADDR__/127.0.0.1}
      if [[ "$role" == peer ]]; then
        rendered=${rendered//:43101/:43201}
        rendered=${rendered//port = 19433/port = 19434}
      fi
      printf '%s\n' "$rendered" >> "$output"
      if [[ "$rendered" =~ ^[[:space:]]*env[[:space:]]*=[[:space:]]*\[$ ]]; then
        printf '  "AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=%s",\n' "$auth" >> "$output"
        printf '  "AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN=%s",\n' "$invitation" >> "$output"
        if [[ "$role" == local ]]; then
          printf '  "AURA_BOOTSTRAP_BROKER_BIND=127.0.0.1:43190",\n' >> "$output"
        else
          printf '  "AURA_BOOTSTRAP_BROKER_URL=http://127.0.0.1:43190",\n' >> "$output"
        fi
      fi
    done
  }
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == '[[instances]]' ]]; then
      if (( in_instance )); then _single_host_emit || return; fi
      in_instance=1
      block=("$line")
    elif (( in_instance )); then
      block+=("$line")
    else
      printf '%s\n' "$line" >> "$output"
      if [[ "$line" == '[run]' ]]; then printf 'fixed_ports = true\n' >> "$output"; fi
    fi
  done < "$template"
  if (( in_instance )); then _single_host_emit || return; fi
  unset -f _single_host_emit
  [[ "$selected" == 1 ]] || return 1
}
