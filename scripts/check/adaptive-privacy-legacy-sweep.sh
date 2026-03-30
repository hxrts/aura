#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

declare -a labels=(
  "transport_hint"
  "mailbox_terms"
  "relay_route_terms"
)

declare -a patterns=(
  'TransportHint'
  'mailbox|mailboxes|mailbox-poll|mailbox_poll'
  'relay-first|relay selection|relay candidate|same-home|neighborhood-hop|guardian relay|friend relay'
)

search_roots=(crates docs work scripts)

for i in "${!labels[@]}"; do
  label="${labels[$i]}"
  pattern="${patterns[$i]}"
  count="$(rg -n "$pattern" "${search_roots[@]}" --glob '!target' | wc -l | tr -d ' ')"
  echo "${label}: ${count}"
done
