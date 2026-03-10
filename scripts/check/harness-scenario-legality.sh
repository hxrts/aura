#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "harness-scenario-legality: $*" >&2
  exit 1
}

inventory="scenarios/harness_inventory.toml"
[[ -f "$inventory" ]] || fail "missing inventory: $inventory"

bash scripts/check/harness-shared-scenario-contract.sh

entries=()
while IFS= read -r line; do
  entries+=("$line")
done < <(
  awk '
    /^\[\[scenario\]\]/ { path=""; class="" }
    /^path = / { path=$3; gsub(/"/, "", path) }
    /^classification = / { class=$3; gsub(/"/, "", class) }
    /^notes = / {
      if (path != "" && class != "") print path "|" class
    }
  ' "$inventory"
)

[[ ${#entries[@]} -gt 0 ]] || fail "no inventory entries found"

for entry in "${entries[@]}"; do
  path=${entry%%|*}
  classification=${entry##*|}
  [[ -f "$path" ]] || fail "missing scenario file: $path"

  if rg -n '^\s*actor\s*=\s*"(tui|web|browser|local)"' "$path" >/tmp/harness-scenario-legality-actors.$$ 2>/dev/null; then
    cat /tmp/harness-scenario-legality-actors.$$ >&2
    rm -f /tmp/harness-scenario-legality-actors.$$
    fail "scenario binds frontend type into actor identity: $path"
  fi
  rm -f /tmp/harness-scenario-legality-actors.$$ || true

  case "$classification" in
    shared)
      if rg -n '^\s*(selector|label|field_id|control_id|pattern)\s*=' "$path" >/tmp/harness-scenario-legality-shared.$$ 2>/dev/null; then
        cat /tmp/harness-scenario-legality-shared.$$ >&2
        rm -f /tmp/harness-scenario-legality-shared.$$
        fail "shared scenario contains renderer-specific mechanics: $path"
      fi
      rm -f /tmp/harness-scenario-legality-shared.$$ || true
      ;;
    tui_only|web_only|to_be_removed)
      ;;
    *)
      fail "unknown scenario classification '$classification' for $path"
      ;;
  esac
done

echo "harness scenario legality: clean"
