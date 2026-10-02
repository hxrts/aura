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
harness_consumers="$(ps -axo comm= | awk '{n=$1;sub(/^.*\//,"",n);if(n~/^(tool_repl|aura-harness)$/)print n}')"
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
  # A live build may unlink a temporary artifact during this read-only walk.
  size="$(du -sk "$path" 2>/dev/null | awk 'NR == 1 {print $1}' || true)"
  [[ "$size" =~ ^[0-9]+$ ]] || size=unavailable
  age=$(((now - modified) / 86400))
  (( age >= 0 )) || age=0
  lane_name="${path##*/}"
  if [[ "$lane_name" == release && -n "$harness_consumers" ]]; then
    use=harness-consumer
  elif [[ "$open_scan" == available ]] && rg -F -q "$path" <<< "$open_files"; then
    use=open-files
  elif rg -F -q "$path" <<< "$process_args"; then
    use=process-reference
  elif [[ -n "$builders" ]]; then
    use=builder-active
  elif [[ "$open_scan" == available ]]; then
    use=idle
  else
    use=unknown
  fi
  printf '%s\t%s\t%s\t%s\n' "$size" "$age" "$use" "$lane_name"
done
