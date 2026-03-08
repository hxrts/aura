#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

scenario_dir="scenarios/harness"

if [[ ! -d "$scenario_dir" ]]; then
  echo "missing scenario directory: $scenario_dir" >&2
  exit 1
fi

semantic_files=()
legacy_files=()

while IFS= read -r file; do
  if rg -q '^(schema_version|execution_mode|required_capabilities)\s*=' "$file"; then
    legacy_files+=("$file")
  else
    semantic_files+=("$file")
  fi
done < <(find "$scenario_dir" -maxdepth 1 -name '*.toml' | sort)

cat <<REPORT
Harness migration audit
=======================
semantic_scenarios=${#semantic_files[@]}
legacy_scenarios=${#legacy_files[@]}

Semantic scenario files:
$(printf '  - %s\n' "${semantic_files[@]}")

Legacy scenario files pending conversion:
$(printf '  - %s\n' "${legacy_files[@]}")
REPORT

legacy_executor_refs=()
while IFS= read -r match; do
  legacy_executor_refs+=("$match")
done < <(rg -n 'itf_replay|tui-itf-trace|tui_state_machine\.qnt' crates scripts docs verification \
  -g '!scripts/check/harness-migration-audit.sh' || true)

echo
echo "Legacy executor references:"
if ((${#legacy_executor_refs[@]} == 0)); then
  echo "  - none"
else
  printf '  - %s\n' "${legacy_executor_refs[@]}"
fi
