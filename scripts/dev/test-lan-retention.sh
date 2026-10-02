#!/usr/bin/env bash
# Exercise the tracked LAN driver lifecycle against an isolated fake tool_repl.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-lan-retention-test.XXXXXX")"
test_root="$(cd "$test_root" && pwd -P)"
driver="$repo_root/scripts/harness/lan/drv.sh"
export AURA_E2E_ROOT="$test_root"
export AURA_E2E_RUN_DIR="$test_root/.tmp/e2e/run"
export AURA_E2E_TOOL_REPL="$test_root/fake-tool-repl.sh"
export AURA_E2E_RUN_TOKEN=lan-retention-test-1
mkdir -p "$test_root/scripts/dev" "$test_root/configs"
cp "$repo_root/scripts/dev/retain-e2e-runs.sh" "$test_root/scripts/dev/retain-e2e-runs.sh"
cat > "$AURA_E2E_TOOL_REPL" <<'EOF'
#!/usr/bin/env bash
exec sleep 30
EOF
chmod +x "$AURA_E2E_TOOL_REPL"
cat > "$test_root/configs/lan.toml" <<'EOF'
[run]
name = "test"
artifact_dir = ".tmp/e2e/run/host-a/artifacts"
EOF
cleanup() {
  bash "$driver" stop >/dev/null 2>&1 || true
  rm -rf "$test_root"
}
trap cleanup EXIT

bash "$driver" start "$test_root/configs/lan.toml" >/dev/null
runs="$test_root/.tmp/e2e/run/host-a/artifacts/runs"
manifest="$runs/$AURA_E2E_RUN_TOKEN/.aura-retention.json"
[[ "$(jq -r .state "$manifest")" == active ]]
if bash "$driver" finish success >/dev/null 2>&1; then
  echo 'LAN run finished while tool_repl was active' >&2; exit 1
fi
bash "$driver" stop >/dev/null
bash "$driver" finish success >/dev/null
[[ "$(jq -r .outcome "$manifest")" == success ]]
if bash "$driver" start "$test_root/configs/lan.toml" >/dev/null 2>&1; then
  echo 'LAN driver reused a completed run bundle' >&2; exit 1
fi

export AURA_E2E_RUN_TOKEN=lan-retention-test-2
bash "$driver" start "$test_root/configs/lan.toml" >/dev/null
bash "$driver" stop >/dev/null
bash "$driver" finish failed >/dev/null
[[ "$(jq -r .outcome "$runs/$AURA_E2E_RUN_TOKEN/.aura-retention.json")" == failed ]]
export AURA_E2E_RUN_TOKEN=short
if bash "$driver" start "$test_root/configs/lan.toml" >/dev/null 2>&1; then
  echo 'LAN driver accepted a token too short for the native harness' >&2; exit 1
fi
[[ ! -e "$runs/short" ]]
echo 'LAN retention lifecycle tests passed'
