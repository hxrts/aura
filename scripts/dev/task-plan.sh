#!/usr/bin/env bash
# Task-number based maintenance for the coordinator's Markdown plan.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
plan="$repo_root/work/8.md"
if [[ ${1:-} == --plan ]]; then plan="$2"; shift 2; fi
command_name="${1:-}"; task="${2:-}"
[[ "$task" =~ ^[0-9]+$ ]] || { echo 'usage: task-plan.sh [--plan FILE] show|log|archive TASK [FILE|KEEP]' >&2; exit 2; }
[[ -f "$plan" ]] || { echo "missing plan: $plan" >&2; exit 2; }
case "$command_name" in
  show) ;;
  log) [[ -s ${3:-} ]] || { echo 'log requires a nonempty entry file' >&2; exit 2; } ;;
  archive) [[ ${3:-3} =~ ^[1-9][0-9]*$ ]] || { echo 'archive requires a positive retained-entry count' >&2; exit 2; } ;;
  *) echo 'expected show, log, or archive' >&2; exit 2 ;;
esac
plan_dir="$(cd "$(dirname "$plan")" && pwd)"
plan="$plan_dir/$(basename "$plan")"
scratch="$(mktemp -d "$plan_dir/.task-plan.XXXXXX")"
# Retain failed preparation for diagnosis; successful writes retain the backup.
output="$scratch/plan.md"
archive="$scratch/evidence.md"
entry="${3:-}"
keep="${3:-3}"
if [[ "$command_name" != show ]]; then
  cp -p "$plan" "$scratch/before.md"
  cmp -s "$plan" "$scratch/before.md"
fi
awk -v action="$command_name" -v task="$task" -v entry="$entry" \
  -v keep="$keep" -v archive="$archive" '
  function header_id(line, id) {
    id=line; sub(/^- \[[ x]\] /,"",id); sub(/\..*$/,"",id); return id
  }
  function print_entry(n, text) {
    n=0
    while((getline text < entry)>0) print (n++ ? "      " : "    - ") text
    close(entry)
  }
  { lines[NR]=$0 }
  END {
    for (i=1;i<=NR;i++) {
      if (lines[i] ~ /^- \[[ x]\] [0-9]+\./) {
        if (header_id(lines[i]) == task) { start=i; found++ }
        else if (start && !stop) stop=i
      }
    }
    if (found != 1) { print "expected exactly one task " task > "/dev/stderr"; exit 2 }
    if (!stop) stop=NR+1
    if (action == "show") { for(i=start;i<stop;i++) print lines[i]; exit }
    for(i=start+1;i<stop;i++) if(lines[i] == "  - **Work log**") { log_line=i; break }
    if(action == "log") {
      insertion=stop
      if(log_line) for(i=log_line+1;i<stop;i++) {
        if(lines[i] ~ /^  - / || lines[i] ~ /^#/) { insertion=i; break }
      }
      for(i=1;i<=NR;i++) {
        if(log_line && i==insertion) print_entry()
        print lines[i]
        if(!log_line && i==start) {
          print "  - **Work log**"
          print_entry()
        }
      }
      if(log_line && insertion==NR+1) print_entry()
      exit
    }
    if(!log_line) { print "task has no Work log" > "/dev/stderr"; exit 2 }
    end=stop
    for(i=log_line+1;i<stop;i++) {
      if(lines[i] ~ /^  - / || lines[i] ~ /^#/) { end=i; break }
      if(lines[i] ~ /^    - /) { begin[++count]=i }
    }
    if(count<=keep) { for(i=1;i<=NR;i++) print lines[i]; exit }
    cutoff=begin[count-keep+1]
    print lines[start] > archive
    print "\n  - **Archived Work log**" > archive
    for(i=log_line+1;i<cutoff;i++) print lines[i] > archive
    close(archive)
    for(i=1;i<=NR;i++) {
      if(i==log_line+1) print "    - Historical receipts: [task " task " archive](task-archives/ARCHIVE_NAME)."
      if(i<=log_line || i>=cutoff) print lines[i]
    }
  }
' "$plan" > "$output"
if [[ "$command_name" == show ]]; then cat "$output"; exit 0; fi
if cmp -s "$plan" "$output"; then echo "task $task unchanged; preparation: $scratch"; exit 0; fi
if [[ -s "$archive" ]]; then
  mkdir -p "$plan_dir/task-archives"
  name="task-$task-$(basename "$scratch").md"
  cp "$archive" "$plan_dir/task-archives/$name"
  # The generated name contains only mktemp characters and the numeric task id.
  sed "s/ARCHIVE_NAME/$name/" "$output" > "$scratch/linked.md"
  mv "$scratch/linked.md" "$output"
fi
if ! cmp -s "$plan" "$scratch/before.md"; then
  echo "plan changed during preparation; preserved candidate and backup in $scratch" >&2
  exit 3
fi
mv "$output" "$plan"
printf 'Updated task %s; original backup: %s\n' "$task" "$scratch/before.md"
