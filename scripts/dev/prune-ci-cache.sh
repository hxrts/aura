#!/usr/bin/env bash
# Cargo-aware CI cache pruning. Never touches .tmp, harness bundles, or data.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
root="$repo_root"
mode=dry
cap_gib="${AURA_BUILD_TARGET_CAP_GIB:-24}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --root) root="${2:?missing root}"; shift 2 ;;
    --dry-run) mode=dry; shift ;;
    --apply) mode=apply; shift ;;
    *) echo 'usage: prune-ci-cache.sh [--root PATH] [--dry-run|--apply]' >&2; exit 2 ;;
  esac
done
root="$(cd "$root" && pwd -P)"
[[ -f "$root/Cargo.toml" ]] || { echo "prune-ci-cache: no Cargo.toml in $root" >&2; exit 2; }
[[ "$cap_gib" =~ ^[0-9]+$ && "$cap_gib" -gt 0 ]] || { echo 'prune-ci-cache: invalid cap' >&2; exit 2; }
if [[ -L "$root/target" ]]; then
  echo 'prune-ci-cache: target is a symlink; refusing' >&2
  exit 1
fi

free_kib() { df -Pk "$root" | awk 'NR == 2 {print $4}'; }
target_kib() {
  if [[ -d "$root/target" ]]; then du -sk "$root/target" | awk 'NR == 1 {print $1}';
  else printf '0\n'; fi
}
preview_sweep() {
  local preview rc
  preview="$(mktemp "${TMPDIR:-/tmp}/aura-ci-sweep-preview.XXXXXX")"
  if (cd "$root" && cargo sweep --maxsize "${cap_gib}GiB" --dry-run -v .) > "$preview" 2>&1; then
    rg '\[INFO\]' "$preview" || cat "$preview"
  else
    rc=$?
    cat "$preview" >&2
    rm -f "$preview"
    return "$rc"
  fi
  release_candidates="$(rg -c 'Would remove: .*target/(release/|wasm-release/|wasm32-unknown-unknown/release/)' "$preview" || true)"
  release_candidates="${release_candidates:-0}"
  printf 'Protected production/web candidates: %s\n' "$release_candidates"
  rm -f "$preview"
}
consumers() {
  ps -axo pid=,comm= | awk '
    {
      name=$2; sub(/^.*\//, "", name);
      if (name ~ /^(cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep|tool_repl|aura-harness|aura)$/)
        printf "%s(%s) ", name, $1
    }'
}
target_has_open_files() {
  local open_files
  if ! command -v lsof >/dev/null; then
    echo 'CI cache: lsof unavailable; treating target as in use' >&2
    return 0
  fi
  if ! open_files="$(lsof -n -P 2>/dev/null)"; then
    echo 'CI cache: open-file scan failed; treating target as in use' >&2
    return 0
  fi
  rg -F -q "$root/target/" <<< "$open_files"
}
printf 'CI cache: mode=%s root=%s cap=%s GiB; free=%s KiB target=%s KiB\n' \
  "$mode" "$root" "$cap_gib" "$(free_kib)" "$(target_kib)"

if [[ "$mode" == dry ]]; then
  preview_sweep
  if (( release_candidates > 0 )) || target_has_open_files; then
    for lane in wasm-debug dylint debug; do
      bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$root" --lane "$lane" --dry-run
    done
  fi
  exit 0
fi

mkdir -p "$root/target"
lock_dir="$root/target/.aura-build-budget.lock"
if ! mkdir "$lock_dir" 2>/dev/null; then
  echo 'prune-ci-cache: another budgeted build holds the target lock' >&2
  exit 1
fi
printf '%s\n' "$$" > "$lock_dir/pid"
cleanup() { rm -f "$lock_dir/pid"; rmdir "$lock_dir" 2>/dev/null || true; }
trap cleanup EXIT

found="$(consumers)"
if [[ -n "$found" ]]; then
  echo "prune-ci-cache: active builder or harness consumer: $found" >&2
  exit 1
fi
printf 'CI cache candidates (preview):\n'
preview_sweep
found="$(consumers)"
if [[ -n "$found" ]]; then
  echo "prune-ci-cache: builder or harness consumer started: $found" >&2
  exit 1
fi
if (( release_candidates > 0 )) || target_has_open_files; then
  echo 'CI cache: global sweep could evict release artifacts or open files; using idle whole lanes'
  for lane in wasm-debug dylint debug; do
    if ! bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$root" \
      --lane "$lane" --apply --lock-owned-by "$$"; then
      echo "CI cache: $lane remains protected or busy" >&2
    fi
    found="$(consumers)"
    [[ -z "$found" ]] || { echo "CI cache: builder or harness consumer started: $found" >&2; exit 1; }
    if (( $(target_kib) <= cap_gib * 1024 * 1024 || $(free_kib) >= 20 * 1024 * 1024 )); then
      break
    fi
  done
else
  (cd "$root" && cargo sweep --maxsize "${cap_gib}GiB" .)
fi
printf 'CI cache after: free=%s KiB target=%s KiB\n' "$(free_kib)" "$(target_kib)"
