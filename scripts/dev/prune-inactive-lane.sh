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
    --assert-idle) mode=assert; shift ;;
    --lock-owned-by) lock_owner="${2:?missing owner pid}"; shift 2 ;;
    --) shift; break ;;
    *) echo 'usage: prune-inactive-lane.sh --lane LANE [--root PATH] [--dry-run|--apply|--assert-idle -- COMMAND]' >&2; exit 2 ;;
  esac
done
if [[ "$mode" == assert ]]; then
  [[ $# -gt 0 ]] || { echo 'assert-idle requires a command' >&2; exit 2; }
else
  [[ $# -eq 0 ]] || { echo 'unexpected command without --assert-idle' >&2; exit 2; }
fi
case "$lane" in
  debug) relative=target/debug ;;
  debug-incremental) relative=target/debug/incremental ;;
  trybuild) relative=target/tests/trybuild ;;
  # trybuild keeps one extra tree per explicit --target triple; the host-triple
  # tree duplicates the default tree whenever some lane passed --target.
  trybuild-host-triple)
    host_triple="${AURA_BUILD_TARGET_TRIPLE:-$(rustc -vV 2>/dev/null | awk '/^host: / {print $2}' || true)}"
    [[ "$host_triple" =~ ^[A-Za-z0-9_.-]+$ ]] || { echo 'cannot determine host triple' >&2; exit 2; }
    relative="target/tests/trybuild/$host_triple" ;;
  kani) relative=target/kani ;;
  wasm-debug) relative=target/wasm32-unknown-unknown/debug ;;
  # Cargo [profile.wasm], used by the canonical Dioxus build.
  wasm-profile) relative=target/wasm32-unknown-unknown/wasm ;;
  # Older Cargo invocations used an unqualified target root for profile.wasm.
  wasm-root-profile) relative=target/wasm ;;
  wasm-release) relative=target/wasm32-unknown-unknown/wasm-release ;;
  wasm-host-release) relative=target/wasm-release ;;
  dylint) relative=target/dylint ;;
  release) relative=target/release ;;
  *) echo 'unknown compiler cache lane' >&2; exit 2 ;;
esac
root="$(cd "$root" && pwd -P)"
cleanup_outcome=refused
cleanup_reason=preflight
cleanup_before=null
cleanup_after=null
cleanup_free_after=null
cleanup_owns_lock=0
if [[ -n "${AURA_BUILD_RECEIPT:-}" ]]; then
  source "$repo_root/scripts/dev/build-receipt.sh"
fi
finish_cleanup() {
  local status=$?
  if (( cleanup_owns_lock == 1 )); then rmdir "$lock_dir" 2>/dev/null || true; fi
  if declare -F aura_receipt_record_cleanup >/dev/null; then
    aura_receipt_record_cleanup "$root" guarded-lane "$lane" "$cleanup_outcome" \
      "$cleanup_reason" "$status" "$cleanup_before" "$cleanup_after" null "$cleanup_free_after"
  fi
}
trap finish_cleanup EXIT
refuse_cleanup() { cleanup_reason="$1"; shift; echo "$*" >&2; exit 1; }
[[ -f "$root/Cargo.toml" ]] || { cleanup_reason=invalid-root; echo 'no Cargo.toml in root' >&2; exit 2; }
[[ ! -L "$root/target" && ! -L "$root/target/wasm32-unknown-unknown" ]] || {
  refuse_cleanup symlink 'target path contains a symlink'
}
path="$root/$relative"
if [[ "$lane" == debug-incremental && -L "$root/target/debug" ]]; then
  refuse_cleanup symlink 'debug parent is a symlink'
fi
if [[ "$lane" == trybuild* && -L "$root/target/tests" ]]; then
  refuse_cleanup symlink 'trybuild parent is a symlink'
fi
[[ ! -L "$path" ]] || refuse_cleanup symlink "lane is a symlink: $path"
if [[ ! -d "$path" && "$mode" != assert ]]; then
  cleanup_outcome=skipped; cleanup_reason=absent; cleanup_before=0; cleanup_after=0
  echo "No lane at $path"; exit 0
fi
cleanup_outcome=failed
cleanup_reason=size-measurement
size=0
if [[ -d "$path" ]]; then size="$(du -sk "$path" | awk 'NR == 1 {print $1}')"; fi
[[ ! "$size" =~ ^[0-9]+$ ]] || cleanup_before="$size"
cleanup_outcome=refused
cleanup_reason=preflight
printf 'Lane=%s mode=%s candidate=%s KiB path=%s\n' "$lane" "$mode" "$size" "$path"
if [[ "$mode" == dry ]]; then
  cleanup_outcome=skipped; cleanup_reason=preview; exit 0
fi

command -v lsof >/dev/null || refuse_cleanup open-file-scan-unavailable 'lsof is required to prove this lane is idle'
lock_dir="$root/target/.aura-build-budget.lock"
if [[ -n "$lock_owner" ]]; then
  [[ "$lock_owner" =~ ^[0-9]+$ && "$lock_owner" == "$PPID" && -f "$lock_dir/pid" ]] || {
    refuse_cleanup parent-owner 'invalid parent lock owner'
  }
  [[ "$(cat "$lock_dir/pid")" == "$lock_owner" ]] || refuse_cleanup checkout-lock 'target lock belongs to another process'
else
  mkdir "$lock_dir" 2>/dev/null || refuse_cleanup checkout-lock 'another build or prune holds target lock'
  cleanup_owns_lock=1
fi
# Only this checkout's builders block it; sibling worktrees build independently.
source "$repo_root/scripts/dev/scoped-builders.sh"
builders="$(aura_scoped_processes "$root" 'cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep|cargo-kani|kani-driver' "$$")"
[[ -z "$builders" ]] || refuse_cleanup active-builder "builder active: $builders"
if [[ "$lane" == debug || "$lane" == release || "$lane" == wasm-profile || "$lane" == wasm-root-profile || "$lane" == wasm-release || "$lane" == wasm-host-release ]]; then
  consumers="$(aura_scoped_processes "$root" 'tool_repl|aura-harness|aura' "$$")"
  [[ -z "$consumers" ]] || refuse_cleanup active-consumer "harness consumer active: $consumers"
fi
open_files="$(lsof -n -P 2>/dev/null)" || refuse_cleanup open-file-scan-failed 'cannot inspect open files'
# Match with POSIX grep, not rg: outside the Nix shell rg may be absent, and a
# failed pipeline here would read as "no open files" and permit the removal.
# grep must consume the whole list: under pipefail, `grep -q` exiting on the
# first match breaks the pipe for printf and turns the match into a failure.
command -v grep >/dev/null || refuse_cleanup open-file-scan-unavailable 'grep is required to match open files'
if printf '%s\n' "$open_files" | grep -F -- "$path" >/dev/null; then
  refuse_cleanup open-files "lane has open files: $path"
fi
# Inspect again after acquiring the shared lock and checking active users.
[[ ! -L "$path" && ! -L "$root/target" ]] || refuse_cleanup changed-path 'lane changed during preflight'
if [[ "$mode" == apply ]]; then
  [[ -d "$path" ]] || refuse_cleanup changed-path 'lane changed during preflight'
else
  [[ ! -e "$path" || -d "$path" ]] || refuse_cleanup changed-path 'lane is not a directory'
fi
[[ ! -L "$root/target/wasm32-unknown-unknown" ]] || refuse_cleanup changed-path 'wasm parent changed during preflight'
[[ "$lane" != debug-incremental || ! -L "$root/target/debug" ]] || refuse_cleanup changed-path 'debug parent changed during preflight'
[[ "$lane" != trybuild* || ! -L "$root/target/tests" ]] || refuse_cleanup changed-path 'trybuild parent changed during preflight'
if [[ "$mode" == assert ]]; then
  cleanup_outcome=skipped; cleanup_reason=idle-assertion
  guarded_status=0
  "$@" || guarded_status=$?
  if (( guarded_status != 0 )); then
    cleanup_outcome=failed; cleanup_reason=guarded-command
  fi
  exit "$guarded_status"
fi
printf 'Removing whole inactive lane: %s (%s KiB)\n' "$path" "$size"
cleanup_outcome=failed
cleanup_reason=removal
rm -rf -- "$path"
cleanup_outcome=removed
cleanup_reason=success
cleanup_after=0
cleanup_free_after="$(df -Pk "$root" | awk 'NR == 2 {print $4}' || true)"
printf 'Free after: %s KiB\n' "$cleanup_free_after"
[[ "$cleanup_free_after" =~ ^[0-9]+$ ]] || cleanup_free_after=null
