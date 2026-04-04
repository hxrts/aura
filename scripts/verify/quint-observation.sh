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

mkdir -p "$(dirname "$TRACE")" "$(dirname "$SCENARIO")"
quint run --main=semantic_observation_smoke --seed="$SEED" --max-samples=1 --n-traces=1 --max-steps=16 --out-itf="$TRACE" "$SPEC" >/dev/null

mapfile -t steps < <(jq -r '.states[].phase["#bigint"]' "$TRACE")
expected=(0 1 2 3 4 5 6 7 8)
if [[ "${steps[*]}" != "${expected[*]}" ]]; then
  echo "error: unexpected Quint smoke trace shape: ${steps[*]}" >&2
  exit 1
fi

cat > "$SCENARIO" <<'SCENARIO_EOF'
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

echo "wrote $SCENARIO from Quint trace $TRACE"
