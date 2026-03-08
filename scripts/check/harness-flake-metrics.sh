#!/usr/bin/env bash
set -euo pipefail

root="${1:-artifacts/harness}"

if ! command -v jq >/dev/null 2>&1; then
  echo "harness-flake-metrics: jq is required" >&2
  exit 1
fi

if [ ! -d "$root" ]; then
  echo "harness-flake-metrics: no artifacts directory at $root"
  exit 0
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

find "$root" -type f -name scenario_report.json -print0 | while IFS= read -r -d '' file; do
  jq -r --arg file "$file" '
    [
      .scenario_id,
      (.total_duration_ms // 0 | tostring),
      (.completed // false | tostring),
      $file
    ] | @tsv
  ' "$file"
done >"$tmp"

if [ ! -s "$tmp" ]; then
  echo "harness-flake-metrics: no scenario_report.json files under $root"
  exit 0
fi

echo "Harness timing summary ($root)"
echo "scenario_id  runs  failures  min_ms  max_ms  avg_ms  spread_ms"

awk -F '\t' '
{
  id = $1
  dur = $2 + 0
  completed = $3
  runs[id] += 1
  sum[id] += dur
  if (!(id in min) || dur < min[id]) min[id] = dur
  if (!(id in max) || dur > max[id]) max[id] = dur
  if (completed != "true") failures[id] += 1
}
END {
  for (id in runs) {
    avg = int(sum[id] / runs[id])
    spread = max[id] - min[id]
    printf "%s  %d  %d  %d  %d  %d  %d\n", id, runs[id], failures[id] + 0, min[id], max[id], avg, spread
  }
}
' "$tmp" | sort

echo
echo "Potentially flaky scenarios"
awk -F '\t' '
{
  id = $1
  dur = $2 + 0
  completed = $3
  runs[id] += 1
  sum[id] += dur
  if (!(id in min) || dur < min[id]) min[id] = dur
  if (!(id in max) || dur > max[id]) max[id] = dur
  if (completed != "true") failures[id] += 1
}
END {
  found = 0
  for (id in runs) {
    spread = max[id] - min[id]
    if (failures[id] > 0 || (runs[id] >= 2 && spread > 1000)) {
      found = 1
      printf "%s: runs=%d failures=%d spread_ms=%d\n", id, runs[id], failures[id] + 0, spread
    }
  }
  if (!found) {
    print "none"
  }
}
' "$tmp" | sort
