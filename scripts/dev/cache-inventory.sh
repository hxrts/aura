#!/usr/bin/env bash
# Read-only inventory of top-level Cargo/Dioxus target lanes.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
root="$(cd "${1:-$repo_root}" && pwd)"
target="$root/target"
[[ -d "$target" && ! -L "$target" ]] || { echo "No ordinary target directory: $target"; exit 0; }

active="$(ps -axo pid=,comm= | awk '{n=$2;sub(/^.*\//,"",n);if(n~/^(cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep|tool_repl|aura-harness|aura)$/)printf "%s(%s) ",n,$1}')"
if [[ -n "$active" ]]; then state="busy: $active"; else state=idle; fi
builders="$(ps -axo comm= | awk '{n=$1;sub(/^.*\//,"",n);if(n~/^(cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep)$/)print n}')"
process_args="$(ps -axo args=)"
open_files=''
open_scan=unavailable
if command -v lsof >/dev/null && open_files="$(lsof -n -P 2>/dev/null)"; then
  open_scan=available
fi
printf 'Target cache: %s\n' "$target"
printf 'Active-use guard: %s\n' "$state"
printf 'Open-file scan: %s\n' "$open_scan"
printf 'Allocated KiB\tAge days\tUse\tLane\n'
now="$(date +%s)"
for path in "$target"/*; do
  [[ -d "$path" && ! -L "$path" ]] || continue
  modified="$(stat -f %m "$path" 2>/dev/null || true)"
  if [[ ! "$modified" =~ ^[0-9]+$ ]]; then
    modified="$(stat -c %Y "$path")"
  fi
  size="$(du -sk "$path" | awk 'NR == 1 {print $1}')"
  age=$(((now - modified) / 86400))
  (( age >= 0 )) || age=0
  if [[ "$open_scan" == available ]] && printf '%s\n' "$open_files" | rg -F -q "$path"; then
    use=open-files
  elif printf '%s\n' "$process_args" | rg -F -q "$path"; then
    use=process-reference
  elif [[ -n "$builders" ]]; then
    use=builder-active
  elif [[ "$open_scan" == available ]]; then
    use=idle
  else
    use=unknown
  fi
  printf '%s\t%s\t%s\t%s\n' "$size" "$age" "$use" "${path##*/}"
done
