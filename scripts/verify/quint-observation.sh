#!/usr/bin/env bash
# Generate a Quint semantic observation trace and convert it to a harness scenario.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SPEC="${1:-$ROOT/verification/quint/harness/semantic_observation_smoke.qnt}"
TRACE="${2:-$ROOT/verification/quint/traces/semantic_observation_smoke.itf.json}"
SCENARIO="${3:-$ROOT/scenarios/harness/quint-semantic-observation-smoke.toml}"
SEED="${QUINT_TRACE_SEED:-424242}"

command -v quint >/dev/null 2>&1 || {
  echo "error: quint not found in PATH" >&2
  exit 1
}
command -v jq >/dev/null 2>&1 || {
  echo "error: jq not found in PATH" >&2
  exit 1
}

# Each output has its own same-directory stage. Publication is two independent
# atomic replacements; it is not an atomic two-artifact transaction.
[[ "$TRACE" != "$SCENARIO" ]] || { echo 'error: trace and scenario paths must differ' >&2; exit 2; }
[[ ! -d "$TRACE" && ! -d "$SCENARIO" ]] || { echo 'error: output destination is a directory' >&2; exit 2; }
mkdir -p "$(dirname "$TRACE")" "$(dirname "$SCENARIO")"
trace_identity="$(cd "$(dirname "$TRACE")" && pwd -P)/$(basename "$TRACE")"
scenario_identity="$(cd "$(dirname "$SCENARIO")" && pwd -P)/$(basename "$SCENARIO")"
[[ "$trace_identity" != "$scenario_identity" ]] || { echo 'error: trace and scenario destinations alias' >&2; exit 2; }
trace_stage=''
scenario_stage=''
trap '[[ -z "$trace_stage" ]] || rm -rf -- "$trace_stage"; [[ -z "$scenario_stage" ]] || rm -rf -- "$scenario_stage"' EXIT
trace_stage="$(mktemp -d "$(dirname "$TRACE")/.aura-quint-observation.XXXXXX")"
scenario_stage="$(mktemp -d "$(dirname "$SCENARIO")/.aura-quint-observation.XXXXXX")"
QUINT_TRACE_SEED="$SEED" QUINT_TRACE_MAIN=semantic_observation_smoke QUINT_TRACE_MAX_STEPS=16 \
  bash "$ROOT/scripts/verify/quint-trace.sh" generate "$SPEC" "$trace_stage/trace.json" > /dev/null

# Observe jq's status directly: a process-substitution failure is not the status
# of the surrounding read/mapfile operation.
steps="$(jq -er '[.states[].phase["#bigint"]] | map(tostring) | join(" ")' "$trace_stage/trace.json")"
if [[ "$steps" != '0 1 2 3 4 5 6 7 8' ]]; then
  echo "error: unexpected Quint smoke trace shape: $steps" >&2
  exit 1
fi

cat > "$scenario_stage/scenario.toml" <<'SCENARIO_EOF'
id = "quint-semantic-observation-smoke"
goal = "Execute a Quint-originated semantic observation flow through aura-harness."

[[steps]]
id = "launch"
action = "launch_actors"
timeout_ms = 15000

[[steps]]
id = "alice-onboarding-visible"
action = "control_visible"
actor = "alice"
control_id = "onboarding_root"
timeout_ms = 30000

[[steps]]
id = "alice-fill-account-name"
action = "fill"
actor = "alice"
field_id = "account_name"
value = "Alice"
timeout_ms = 3000

[[steps]]
id = "alice-submit-account"
action = "activate"
actor = "alice"
control_id = "onboarding_create_account_button"
timeout_ms = 3000

[[steps]]
id = "alice-neighborhood-visible"
action = "screen_is"
actor = "alice"
screen_id = "neighborhood"
timeout_ms = 30000

[[steps]]
id = "alice-neighborhood-ready"
action = "readiness_is"
actor = "alice"
readiness = "ready"
timeout_ms = 30000

[[steps]]
id = "alice-open-settings-via-nav"
action = "navigate"
actor = "alice"
screen_id = "settings"
timeout_ms = 3000

[[steps]]
id = "alice-settings-visible"
action = "screen_is"
actor = "alice"
screen_id = "settings"
timeout_ms = 30000
SCENARIO_EOF

mv -f "$trace_stage/trace.json" "$TRACE"
if mv -f "$scenario_stage/scenario.toml" "$SCENARIO"; then
  :
else
  publication_failure=$?
  echo "error: trace published at $TRACE; scenario publication failed at $SCENARIO (outputs are not an atomic pair)" >&2
  exit "$publication_failure"
fi

echo "wrote $SCENARIO from Quint trace $TRACE"
