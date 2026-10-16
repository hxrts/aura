#!/usr/bin/env bash
# Caller-owned, one-shot wakeup reader; receipts retain the durable event log.
set -euo pipefail
shared="${AURA_BUILD_SHARED_DIR:-$HOME/.cache/aura-build}"
subscribers="$shared/event-subscribers"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repo/scripts/dev/build-receipt.sh"
mode() { aura_receipt_stat_mode "$1"; }
owner() { aura_receipt_stat_owner "$1"; }
private_parent() {
  [[ -d "$shared" && ! -L "$shared" && "$(owner "$shared")" == "$(id -u)" ]] || exit 2
  shared_mode="$(mode "$shared")"
  [[ "$shared_mode" =~ ^[0-7]{3,4}$ ]] && (( (8#$shared_mode & 8#022) == 0 )) || exit 2
  mkdir -p -m 700 "$subscribers"
  [[ -d "$subscribers" && ! -L "$subscribers" && "$(owner "$subscribers")" == "$(id -u)" &&
    "$(mode "$subscribers")" == 700 ]] || exit 2
}
valid_fifo() {
  local fifo="$1"
  local parent="$(dirname "$fifo")"
  [[ "$(dirname "$parent")" == "$subscribers" &&
    "$(basename "$parent")" =~ ^listener\.[A-Za-z0-9]+$ &&
    "$(basename "$fifo")" == event.fifo &&
    -d "$parent" && ! -L "$parent" && -p "$fifo" && ! -L "$fifo" &&
    "$(owner "$parent")" == "$(id -u)" && "$(owner "$fifo")" == "$(id -u)" &&
    "$(mode "$parent")" == 700 && "$(mode "$fifo")" == 600 ]]
}
valid_receipt() {
  local receipt="$1" directory receipts root
  directory="$(dirname "$receipt")"
  receipts="$(dirname "$directory")"
  [[ "$(basename "$receipt")" == receipt.json &&
    "$(basename "$directory")" =~ ^run\.[A-Za-z0-9]+$ &&
    -f "$receipt" && ! -L "$receipt" && ! -L "$directory" && ! -L "$receipts" &&
    "$(owner "$receipt")" == "$(id -u)" && "$(owner "$directory")" == "$(id -u)" &&
    "$(mode "$receipt")" == 600 && "$(mode "$directory")" == 700 ]] || return 1
  root="$(jq -r '.root // empty' "$receipt" 2>/dev/null || true)"
  [[ -n "$root" && "$receipts" == "$root/artifacts/disk-budget/receipts" ]]
}
case "${1:-}" in
  --create)
    [[ $# == 1 ]] || exit 2
    private_parent
    directory="$(mktemp -d "$subscribers/listener.XXXXXX")"
    chmod 700 "$directory"
    umask 077
    mkfifo -m 600 "$directory/event.fifo"
    printf '%s\n' "$directory/event.fifo"
    ;;
  --next)
    [[ $# == 2 ]] || exit 2
    private_parent
    valid_fifo "$2" || exit 2
    IFS= read -r event < "$2" || exit 1
    receipt="$(jq -r '.receipt // empty' <<< "$event")"
    valid_receipt "$receipt" || exit 1
    jq -e --argjson wake "$event" '
      .source.fingerprint as $source |
      any(.events[]?; .sequence==$wake.sequence and .kind==$wake.kind and
        .source_fingerprint==$wake.source_fingerprint and
        .source_fingerprint==$source)' "$receipt" >/dev/null || exit 1
    jq -c --arg receipt "$receipt" --argjson wake "$event" '
      .events[] | select(.sequence==$wake.sequence) |
      {sequence,kind,source_fingerprint,exit_status,failure} + {receipt:$receipt}' "$receipt"
    ;;
  --recover)
    [[ $# == 3 && "$3" =~ ^[0-9]+$ ]] || exit 2
    receipt="$2"
    valid_receipt "$receipt" || exit 2
    jq -c --arg receipt "$receipt" --argjson after "$3" '
      .source.fingerprint as $source |
      .events[]? | select(.sequence>$after) |
      {sequence,kind,source_fingerprint,exit_status,failure} + {receipt:$receipt} |
      select(.source_fingerprint==$source)' "$receipt"
    ;;
  --close)
    [[ $# == 2 ]] || exit 2
    private_parent
    valid_fifo "$2" || exit 2
    rm "$2"
    rmdir "$(dirname "$2")"
    ;;
  *) echo 'usage: receipt-events.sh --create | --next FIFO | --recover RECEIPT AFTER_SEQUENCE | --close FIFO' >&2; exit 2 ;;
esac
