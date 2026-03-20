#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

readonly workflow_root="crates/aura-app/src/workflows"
readonly legacy_pattern='OWNERSHIP: (view-write-legacy|view-read-for-decision|fallback-heuristic)'
readonly final_pattern='OWNERSHIP: (observed|observed-display-update|authoritative-source|first-run-default|fact-backed|test-only-helper)'
readonly sensitive_pattern='with_(chat|homes|contacts|recovery|neighborhood)_state|views_mut\(|chat_snapshot\(|contacts_snapshot\(|recovery_snapshot\(|core\.snapshot\(|snapshot\(\)'

if rg -n --no-heading "$legacy_pattern" "$workflow_root" >&2; then
  echo "legacy workflow ownership tags are no longer allowed" >&2
  exit 1
fi

if rg -n --no-heading 'OWNERSHIP: deprecated-legacy-bridge' "$workflow_root" >&2; then
  echo "deprecated workflow bridge tags are no longer allowed" >&2
  exit 1
fi

violations=()
while IFS= read -r hit; do
  file="${hit%%:*}"
  rest="${hit#*:}"
  line="${rest%%:*}"

  if [[ "$file" == "crates/aura-app/src/workflows/mod.rs" ]]; then
    continue
  fi

  if awk -v limit="$line" '
      NR <= limit && /^\#\[cfg\(test\)\]/ { test_start = NR }
      END { exit !(test_start != 0 && limit >= test_start) }
    ' "$file"; then
    continue
  fi

  if ! awk -v limit="$line" -v pattern="$final_pattern" '
      NR >= limit - 60 && NR <= limit && $0 ~ pattern { found = 1 }
      END { exit !found }
    ' "$file"; then
    violations+=("$hit")
  fi
done < <(rg -n --no-heading "$sensitive_pattern" "$workflow_root")

if (( ${#violations[@]} > 0 )); then
  echo "projection-sensitive workflow sites must carry a final ownership classification:" >&2
  printf '%s\n' "${violations[@]}" >&2
  exit 1
fi

classified_count="$(rg -n --no-heading "$final_pattern" "$workflow_root" | wc -l | tr -d ' ')"
echo "workflow ownership audit clean ($classified_count final ownership tags)"
