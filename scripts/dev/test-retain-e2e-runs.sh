#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-retention-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT
runs="$test_root/.tmp/e2e/run/host-a/artifacts/runs"
fakebin="$test_root/fakebin"
mkdir -p "$runs" "$fakebin"
export ACTIVE_FILE="$test_root/active"
export PATH="$fakebin:$PATH"
cat > "$fakebin/ps" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$ACTIVE_FILE" ]] || printf '123 tool_repl\n'
EOF
chmod +x "$fakebin/ps"
for name in s1 s2 s3 failed pinned active legacy; do mkdir "$runs/$name"; done
printf 'evidence\n' > "$runs/failed/events.json"
printf 'runtime log\n' > "$runs/failed/runtime.log"
printf 'UI snapshot\n' > "$runs/failed/ui_state.json"
printf 'outside\n' > "$test_root/outside"
ln -s "$test_root" "$runs/unsafe"

retain() { bash "$repo_root/scripts/dev/retain-e2e-runs.sh" --root "$runs" "$@"; }
expect_status() {
  local wanted="$1" actual
  shift
  if "$@" > "$test_root/output" 2>&1; then actual=0; else actual=$?; fi
  if [[ "$actual" -ne "$wanted" ]]; then
    cat "$test_root/output" >&2
    echo "expected status $wanted, got $actual" >&2
    exit 1
  fi
}
for name in s1 s2 s3; do retain begin "$name" >/dev/null; retain finish "$name" success >/dev/null; done
for pair in 's1 1' 's2 2' 's3 3'; do
  set -- $pair
  jq --argjson epoch "$2" '.finished_epoch=$epoch' "$runs/$1/.aura-retention.json" > "$test_root/edit.json"
  cp "$test_root/edit.json" "$runs/$1/.aura-retention.json"
done
retain begin failed >/dev/null
retain finish failed failed >/dev/null
retain begin pinned >/dev/null
retain finish pinned success >/dev/null
retain pin pinned >/dev/null
retain begin active >/dev/null
if retain finish s1 success >/dev/null 2>&1; then echo 'duplicate finish accepted' >&2; exit 1; fi
expect_status 2 retain begin ../escape

export AURA_E2E_KEEP_SUCCESSES=2
expect_status 0 retain prune --dry-run
rg -q 's1' "$test_root/output"
[[ -d "$runs/s1" && -f "$runs/failed/events.json" && -f "$test_root/outside" ]]

mkdir "$runs/.aura-retention.lock"
expect_status 1 retain pin s1
expect_status 1 retain prune --apply
[[ "$(jq -r .pinned "$runs/s1/.aura-retention.json")" == false ]]
rmdir "$runs/.aura-retention.lock"

touch "$ACTIVE_FILE"
expect_status 1 retain prune --apply
[[ -d "$runs/s1" ]]
rm -f "$ACTIVE_FILE"

expect_status 0 retain prune --apply
[[ ! -e "$runs/s1" ]]
for name in s2 s3 failed pinned active legacy; do [[ -d "$runs/$name" ]]; done
[[ -f "$runs/failed/events.json" && -f "$runs/failed/runtime.log" && -f "$runs/failed/ui_state.json" ]]
[[ -L "$runs/unsafe" && -f "$test_root/outside" ]]

byte_runs="$test_root/.tmp/e2e/run/host-b/artifacts/runs"
mkdir -p "$byte_runs/b1" "$byte_runs/b2"
for name in b1 b2; do
  bash "$repo_root/scripts/dev/retain-e2e-runs.sh" --root "$byte_runs" begin "$name" >/dev/null
  dd if=/dev/zero of="$byte_runs/$name/evidence.bin" bs=1048576 count=2 2>/dev/null
  bash "$repo_root/scripts/dev/retain-e2e-runs.sh" --root "$byte_runs" finish "$name" success >/dev/null
done
jq '.finished_epoch=1' "$byte_runs/b1/.aura-retention.json" > "$test_root/edit.json"
cp "$test_root/edit.json" "$byte_runs/b1/.aura-retention.json"
jq '.finished_epoch=2' "$byte_runs/b2/.aura-retention.json" > "$test_root/edit.json"
cp "$test_root/edit.json" "$byte_runs/b2/.aura-retention.json"
AURA_E2E_KEEP_SUCCESSES=10 AURA_E2E_MAX_SUCCESS_MIB=1 \
  bash "$repo_root/scripts/dev/retain-e2e-runs.sh" --root "$byte_runs" prune --apply >/dev/null
[[ ! -e "$byte_runs/b1" && -d "$byte_runs/b2" ]]

mkdir -p "$test_root/link/.tmp/e2e/run/host-a/artifacts"
ln -s "$runs" "$test_root/link/.tmp/e2e/run/host-a/artifacts/runs"
expect_status 1 bash "$repo_root/scripts/dev/retain-e2e-runs.sh" --root "$test_root/link/.tmp/e2e/run/host-a/artifacts/runs" prune --apply
mkdir -p "$test_root/other/artifacts/runs"
expect_status 2 bash "$repo_root/scripts/dev/retain-e2e-runs.sh" --root "$test_root/other/artifacts/runs" prune --apply
echo 'retain-e2e-runs safety tests passed'
