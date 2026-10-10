#!/usr/bin/env bash
# Execute the actual recipe; isolate only its nested policy command.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
recipe_file="${1:-$repo_root/justfile}"
real_just="$(command -v just)"
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
export FIXTURE_CALLS="$fixture/calls" FIXTURE_STATUS=0
mkdir -p "$fixture/bin"
cat > "$fixture/bin/just" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$FIXTURE_CALLS"
exit "$FIXTURE_STATUS"
EOF
chmod +x "$fixture/bin/just"
export PATH="$fixture/bin:$PATH"
for lane in layers effects deps concurrency invariants workflows; do
  : > "$FIXTURE_CALLS"
  "$real_just" --justfile "$recipe_file" check-arch-lane "$lane" > "$fixture/output" 2>&1
  [[ "$(cat "$FIXTURE_CALLS")" == "_policy-check check arch --$lane" ]]
  : > "$FIXTURE_CALLS"
  status=0
  FIXTURE_STATUS=42 "$real_just" --justfile "$recipe_file" check-arch-lane "$lane" > "$fixture/output" 2>&1 || status=$?
  [[ "$status" == 42 ]] || { echo "retained architecture lane $lane lost failure42 (got$status)" >&2; exit 1; }
  [[ "$(cat "$FIXTURE_CALLS")" == "_policy-check check arch --$lane" ]]
done
for lane in unknown todos completeness; do
  : > "$FIXTURE_CALLS"
  status=0
  "$real_just" --justfile "$recipe_file" check-arch-lane "$lane" > "$fixture/output" 2>&1 || status=$?
  [[ "$status" == 2 && ! -s "$FIXTURE_CALLS" ]]
  grep -q 'Unknown lane:' "$fixture/output"
done
printf 'Architecture lane dispatch fixtures passed: six success, six failure, three refusal\n'
