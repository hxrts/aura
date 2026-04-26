#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp_dir="${repo_root}/.tmp/ci-dry-run-parity"
mkdir -p "$tmp_dir"

justfile="${repo_root}/justfile"
if [[ ! -f "$justfile" ]]; then
  justfile="${repo_root}/Justfile"
fi

local_recipes="${tmp_dir}/local-recipes.txt"
github_recipes="${tmp_dir}/github-recipes.txt"
missing_recipes="${tmp_dir}/missing-recipes.txt"

gawk '
  /^ci-dry-run / { in_dry_run = 1 }
  in_dry_run && /\[\[ "\$profile" == "all" \]\]/ { exit }
  in_dry_run && /add_step/ {
    line = $0
    while (match(line, /(^|[[:space:]\047"])(just)[[:space:]]+([A-Za-z0-9_-]+)/, m)) {
      print m[3]
      line = substr(line, RSTART + RLENGTH)
    }
  }
' "$justfile" | sort -u > "$local_recipes"

: > "$github_recipes"

workflow_runs_on_branch_push() {
  local workflow="$1"
  gawk '
    /^on:/ { in_on = 1; next }
    in_on && /^[A-Za-z0-9_-]+:/ { in_on = 0 }
    in_on && /^  push:/ { in_push = 1; push_seen = 1; next }
    in_push && /^  [A-Za-z0-9_-]+:/ { in_push = 0 }
    in_push && /branches:/ { branch_push = 1 }
    in_push && /tags:/ { tag_push = 1 }
    END {
      if (push_seen && (branch_push || !tag_push)) {
        exit 0
      }
      exit 1
    }
  ' "$workflow"
}

extract_required_push_recipes() {
  local workflow="$1"
  gawk '
    function flush_job() {
      if (job == "") {
        return
      }
      if (block !~ /replay-command:/) {
        return
      }
      if (block ~ /(^|\n)[ ]{4}continue-on-error:[ ]*true/) {
        return
      }
      if (block ~ /(^|\n)[ ]{4}if:[^\n]*(schedule|workflow_dispatch)/ &&
          block !~ /(^|\n)[ ]{4}if:[^\n]*push/) {
        return
      }

      text = block
      while (match(text, /replay-command:[^\n]*/, replay)) {
        replay_start = RSTART
        replay_length = RLENGTH
        line = replay[0]
        while (match(line, /(^|[[:space:]\047"])(just)[[:space:]]+([A-Za-z0-9_-]+)/, recipe)) {
          print recipe[3]
          line = substr(line, RSTART + RLENGTH)
        }
        text = substr(text, replay_start + replay_length)
      }
    }

    /^jobs:/ { in_jobs = 1; next }
    in_jobs && /^  [A-Za-z0-9_-]+:/ {
      flush_job()
      job = $1
      block = $0 "\n"
      next
    }
    in_jobs && job != "" {
      block = block $0 "\n"
    }
    END {
      flush_job()
    }
  ' "$workflow"
}

while IFS= read -r workflow; do
  if workflow_runs_on_branch_push "$workflow"; then
    extract_required_push_recipes "$workflow" >> "$github_recipes"
  fi
done < <(find "${repo_root}/.github/workflows" -maxdepth 1 -type f -name '*.yml' | sort)

sort -u "$github_recipes" -o "$github_recipes"
comm -23 "$github_recipes" "$local_recipes" > "$missing_recipes"

if [[ -s "$missing_recipes" ]]; then
  echo "ci-dry-run push is missing blocking GitHub push replay recipes:" >&2
  sed 's/^/  - /' "$missing_recipes" >&2
  echo >&2
  echo "Add matching just recipes to the ci-dry-run push/all step list, or mark the GitHub job non-blocking." >&2
  exit 1
fi

github_count="$(wc -l < "$github_recipes" | tr -d ' ')"
local_count="$(wc -l < "$local_recipes" | tr -d ' ')"
echo "ci-dry-run push parity ok: ${github_count} blocking GitHub replay recipes covered by ${local_count} local dry-run recipes"
