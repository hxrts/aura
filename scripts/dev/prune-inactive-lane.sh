#!/usr/bin/env bash
# Explicit whole-lane cleanup when cargo-sweep cannot spare live sibling lanes.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
root="$repo_root"
lane=''
mode=dry
lock_owner=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --root) root="${2:?missing root}"; shift 2 ;;
    --lane) lane="${2:?missing lane}"; shift 2 ;;
    --dry-run) mode=dry; shift ;;
    --apply) mode=apply; shift ;;
    --lock-owned-by) lock_owner="${2:?missing owner pid}"; shift 2 ;;
    *) echo 'usage: prune-inactive-lane.sh --lane debug|wasm-debug|dylint|release [--root PATH] [--dry-run|--apply]' >&2; exit 2 ;;
  esac
done
case "$lane" in
  debug) relative=target/debug ;;
  wasm-debug) relative=target/wasm32-unknown-unknown/debug ;;
  dylint) relative=target/dylint ;;
  release) relative=target/release ;;
  *) echo 'lane must be debug, wasm-debug, dylint or release' >&2; exit 2 ;;
esac
root="$(cd "$root" && pwd -P)"
[[ -f "$root/Cargo.toml" ]] || { echo 'no Cargo.toml in root' >&2; exit 2; }
[[ ! -L "$root/target" && ! -L "$root/target/wasm32-unknown-unknown" ]] || {
  echo 'target path contains a symlink' >&2; exit 1;
}
path="$root/$relative"
[[ ! -L "$path" ]] || { echo "lane is a symlink: $path" >&2; exit 1; }
if [[ ! -d "$path" ]]; then echo "No lane at $path"; exit 0; fi
size="$(du -sk "$path" | awk 'NR == 1 {print $1}')"
printf 'Lane=%s mode=%s candidate=%s KiB path=%s\n' "$lane" "$mode" "$size" "$path"
[[ "$mode" == apply ]] || exit 0

command -v lsof >/dev/null || { echo 'lsof is required to prove this lane is idle' >&2; exit 1; }
lock_dir="$root/target/.aura-build-budget.lock"
if [[ -n "$lock_owner" ]]; then
  [[ "$lock_owner" =~ ^[0-9]+$ && "$lock_owner" == "$PPID" && -f "$lock_dir/pid" ]] || {
    echo 'invalid parent lock owner' >&2; exit 1;
  }
  [[ "$(cat "$lock_dir/pid")" == "$lock_owner" ]] || { echo 'target lock belongs to another process' >&2; exit 1; }
else
  mkdir "$lock_dir" 2>/dev/null || { echo 'another build or prune holds target lock' >&2; exit 1; }
  trap 'rmdir "$lock_dir" 2>/dev/null || true' EXIT
fi
builders="$(ps -axo pid=,comm= | awk '{n=$2;sub(/^.*\//,"",n);if(n~/^(cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep)$/)printf "%s(%s) ",n,$1}')"
[[ -z "$builders" ]] || { echo "builder active: $builders" >&2; exit 1; }
if [[ "$lane" == release ]]; then
  consumers="$(ps -axo pid=,comm= | awk '{n=$2;sub(/^.*\//,"",n);if(n~/^(tool_repl|aura-harness|aura)$/)printf "%s(%s) ",n,$1}')"
  [[ -z "$consumers" ]] || { echo "harness consumer active: $consumers" >&2; exit 1; }
fi
open_files="$(lsof -n -P 2>/dev/null)" || { echo 'cannot inspect open files' >&2; exit 1; }
if printf '%s\n' "$open_files" | rg -F "$path"; then
  echo "lane has open files: $path" >&2
  exit 1
fi
# Inspect again after acquiring the shared lock and checking active users.
[[ -d "$path" && ! -L "$path" ]] || { echo 'lane changed during preflight' >&2; exit 1; }
printf 'Removing whole inactive lane: %s (%s KiB)\n' "$path" "$size"
rm -rf -- "$path"
printf 'Free after: %s KiB\n' "$(df -Pk "$root" | awk 'NR == 2 {print $4}')"
