#!/usr/bin/env bash
# Explicit, outcome-aware retention for harness run bundles. Legacy bundles
# without a retention manifest are listed but never deleted automatically.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
root="${AURA_E2E_ARTIFACT_RUNS_ROOT:-$repo_root/.tmp/e2e/run/host-a/artifacts/runs}"
keep_count="${AURA_E2E_KEEP_SUCCESSES:-10}"
max_success_mib="${AURA_E2E_MAX_SUCCESS_MIB:-512}"
mode=dry
action=''
run_name=''
outcome=''

usage() {
  cat <<'EOF'
Usage: retain-e2e-runs.sh [--root RUNS_DIR] begin RUN_ID
       retain-e2e-runs.sh [--root RUNS_DIR] finish RUN_ID success|failed
       retain-e2e-runs.sh [--root RUNS_DIR] pin|unpin RUN_ID
       retain-e2e-runs.sh [--root RUNS_DIR] prune [--dry-run|--apply]

Only completed, unpinned, explicitly classified successful runs are eligible.
The newest success is always kept. Old bundles without .aura-retention.json
are reported for review and preserved.
EOF
}
while [[ $# -gt 0 ]]; do
  case "$1" in
    --root) root="${2:?missing root}"; shift 2 ;;
    begin|finish|pin|unpin|prune)
      action="$1"; shift
      if [[ "$action" != prune ]]; then run_name="${1:?missing run id}"; shift; fi
      if [[ "$action" == finish ]]; then outcome="${1:?missing outcome}"; shift; fi
      ;;
    --dry-run) mode=dry; shift ;;
    --apply) mode=apply; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
done
[[ -n "$action" ]] || { usage >&2; exit 2; }
[[ "$keep_count" =~ ^[0-9]+$ && "$keep_count" -gt 0 ]] || { echo 'invalid keep count' >&2; exit 2; }
[[ "$max_success_mib" =~ ^[0-9]+$ && "$max_success_mib" -gt 0 ]] || { echo 'invalid size budget' >&2; exit 2; }
if [[ "$action" == finish && "$outcome" != success && "$outcome" != failed ]]; then
  echo 'finish outcome must be success or failed' >&2; exit 2
fi
if [[ "$action" != prune && ! "$run_name" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || [[ "$run_name" == *..* ]]; then
  echo 'invalid run id' >&2; exit 2
fi
command -v jq >/dev/null || { echo 'jq is required' >&2; exit 2; }
[[ "$root" == */artifacts/runs ]] || { echo 'run root must end in /artifacts/runs' >&2; exit 2; }
[[ ! -L "$root" ]] || { echo 'run root is a symlink' >&2; exit 1; }
if [[ "$action" == prune && ! -d "$root" ]]; then
  echo "Retention: no run root at $root"
  exit 0
fi
[[ -d "$root" ]] || { echo "missing run root: $root" >&2; exit 1; }
root="$(cd "$root" && pwd -P)"
lock_dir="$root/.aura-retention.lock"
acquire_lock() {
  mkdir "$lock_dir" 2>/dev/null || { echo 'retention: another manifest update or prune holds the lock' >&2; exit 1; }
  trap 'rmdir "$lock_dir" 2>/dev/null || true' EXIT
}

safe_run_dir() {
  local path="$1"
  [[ -d "$path" && ! -L "$path" && "$(cd "$path" && pwd -P)" == "$root/"* ]]
}
write_manifest() {
  local path="$1" data="$2" tmp
  tmp="$(mktemp "$path/.aura-retention.json.XXXXXX")"
  printf '%s\n' "$data" > "$tmp"
  mv -f "$tmp" "$path/.aura-retention.json"
}

if [[ "$action" != prune ]]; then
  acquire_lock
  path="$root/$run_name"
  safe_run_dir "$path" || { echo "unsafe or missing run directory: $path" >&2; exit 1; }
  manifest="$path/.aura-retention.json"
  [[ ! -L "$manifest" ]] || { echo 'retention manifest is a symlink' >&2; exit 1; }
  case "$action" in
    begin)
      [[ ! -e "$manifest" ]] || { echo 'run already classified' >&2; exit 1; }
      data="$(jq -n --arg run "$run_name" --arg started "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        '{schema_version:1,run_id:$run,state:"active",outcome:null,pinned:false,started_at:$started,finished_epoch:null}')"
      ;;
    finish)
      [[ -f "$manifest" ]] || { echo 'run has no begin manifest' >&2; exit 1; }
      data="$(jq --arg outcome "$outcome" --argjson finished "$(date +%s)" \
        'if .schema_version == 1 and .state == "active" then
          .state="completed" | .outcome=$outcome | .finished_epoch=$finished
         else error("run is not active") end' "$manifest")"
      ;;
    pin|unpin)
      [[ -f "$manifest" ]] || { echo 'run has no manifest' >&2; exit 1; }
      if [[ "$action" == pin ]]; then pinned=true; else pinned=false; fi
      data="$(jq --argjson pinned "$pinned" 'if .schema_version == 1 then .pinned=$pinned else error("unknown schema") end' "$manifest")"
      ;;
  esac
  write_manifest "$path" "$data"
  printf '%s: %s\n' "$action" "$path"
  exit 0
fi

scratch="$(mktemp -d "${TMPDIR:-/tmp}/aura-run-retention.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
: > "$scratch/eligible"
: > "$scratch/legacy"
for path in "$root"/*; do
  [[ -e "$path" || -L "$path" ]] || continue
  if ! safe_run_dir "$path"; then
    printf 'unsafe path: %s\n' "$path" >> "$scratch/legacy"
    continue
  fi
  manifest="$path/.aura-retention.json"
  if [[ ! -f "$manifest" || -L "$manifest" ]]; then
    printf 'unclassified: %s\n' "$path" >> "$scratch/legacy"
    continue
  fi
  if ! fields="$(jq -r '[.schema_version,.run_id,.state,.outcome,.pinned,.finished_epoch] | @tsv' "$manifest" 2>/dev/null)"; then
    printf 'invalid manifest: %s\n' "$path" >> "$scratch/legacy"
    continue
  fi
  IFS=$'\t' read -r schema id state result pinned finished <<< "$fields"
  name="${path##*/}"
  if [[ "$schema" != 1 || "$id" != "$name" || "$state" != completed || "$result" != success || "$pinned" != false || ! "$finished" =~ ^[0-9]+$ ]]; then
    printf 'protected: %s (%s/%s/pinned=%s)\n' "$path" "$state" "$result" "$pinned" >> "$scratch/legacy"
    continue
  fi
  size_kib="$(du -sk "$path" | awk 'NR==1 {print $1}')"
  printf '%s\t%s\t%s\n' "$finished" "$size_kib" "$name" >> "$scratch/eligible"
done
sort -t $'\t' -k1,1nr "$scratch/eligible" > "$scratch/sorted"
: > "$scratch/candidates"
kept=0
kept_kib=0
max_kib=$((max_success_mib * 1024))
while IFS=$'\t' read -r finished size_kib name; do
  [[ -n "$name" ]] || continue
  if (( kept == 0 || (kept < keep_count && kept_kib + size_kib <= max_kib) )); then
    kept=$((kept + 1))
    kept_kib=$((kept_kib + size_kib))
  else
    printf '%s\t%s\n' "$size_kib" "$name" >> "$scratch/candidates"
  fi
done < "$scratch/sorted"
printf 'Retention: root=%s mode=%s keep=%s successes max=%s MiB\n' \
  "$root" "$mode" "$keep_count" "$max_success_mib"
printf 'Kept classified successes: %s (%s KiB)\n' "$kept" "$kept_kib"
printf 'Unclassified/protected/unsafe runs: %s\n' "$(wc -l < "$scratch/legacy" | tr -d ' ')"
cat "$scratch/legacy"
printf 'Prune candidates (estimated allocated KiB):\n'
awk -F '\t' -v root="$root" '{total += $1; printf "%s KiB  %s/%s\n", $1, root, $2} END {printf "Total candidate: %s KiB\n", total + 0}' "$scratch/candidates"
[[ "$mode" == apply ]] || exit 0

# A running harness may still read a completed bundle. Require an idle host.
active="$(ps -axo pid=,comm= | awk '{n=$2;sub(/^.*\//,"",n);if(n~/^(tool_repl|aura-harness|aura)$/)printf "%s(%s) ",n,$1}')"
[[ -z "$active" ]] || { echo "retention: harness consumer active: $active" >&2; exit 1; }
acquire_lock
trap 'rmdir "$lock_dir" 2>/dev/null || true; rm -rf "$scratch"' EXIT
while IFS=$'\t' read -r size_kib name; do
  [[ -n "$name" ]] || continue
  path="$root/$name"
  safe_run_dir "$path" || { echo "retention: candidate changed: $path" >&2; exit 1; }
  manifest="$path/.aura-retention.json"
  jq -e --arg name "$name" \
    '.schema_version == 1 and .run_id == $name and .state == "completed" and .outcome == "success" and .pinned == false' \
    "$manifest" >/dev/null || { echo "retention: manifest changed: $path" >&2; exit 1; }
  printf 'Removing %s KiB: %s\n' "$size_kib" "$path"
  rm -rf -- "$path"
done < "$scratch/candidates"
