#!/usr/bin/env bash
# Isolated batch-finalization fixture: no processes, profiles or network access.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
export FIXTURE_ROOT="$fixture"
mkdir -p "$fixture/bin" "$fixture/local/work/handoff" "$fixture/local/scripts/harness/lan" "$fixture/remote root's/scripts/harness/lan" "$fixture/local/.tmp/main-smoke-fixture-commit"
cp "$here/batch.sh" "$here/finish-batch.sh" "$fixture/local/scripts/harness/lan/"
printf '%s\n' fixture-commit > "$fixture/local/.tmp/main-smoke-fixture-commit/validated-commit"
cat > "$fixture/bin/git" <<'MOCK'
#!/usr/bin/env bash
case "$1" in rev-parse) echo fixture-commit;; status) :;; *) exit 2;; esac
MOCK
cat > "$fixture/bin/ssh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
command=${@: -1}
if [[ "$command" == 'bash -s -- '* ]]; then exec bash -c "$command"; fi
echo fixture-commit
MOCK
cat > "$fixture/bin/timeout" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
duration=$1; shift
if [[ "$duration" == 1500 && "$FIXTURE_CASE" == interrupt ]]; then
  trap 'printf stopped > "$FIXTURE_ROOT/batch-stopped"; exit 143' TERM
  mkfifo "$FIXTURE_ROOT/cancel-wait"
  kill -TERM "$PPID"
  read -r ignored < "$FIXTURE_ROOT/cancel-wait"
  exit 99
fi
exec "$@"
MOCK
cat > "$fixture/local/scripts/harness/lan/env.sh" <<'MOCK'
AURA_E2E_REMOTE=fixture-host
AURA_E2E_REMOTE_ROOT="$FIXTURE_ROOT/remote root's"
AURA_E2E_RUN_DIR="$PWD/.tmp/e2e/run"
MOCK
cp "$fixture/local/scripts/harness/lan/env.sh" "$fixture/remote root's/scripts/harness/lan/env.sh"
cat > "$fixture/local/scripts/harness/lan/fresh.sh" <<'MOCK'
#!/usr/bin/env bash
[[ "$FIXTURE_CASE" != failure ]] || exit 7
MOCK
printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/local/work/handoff/lan-checklist.sh"
cat > "$fixture/local/scripts/harness/lan/drv.sh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
host=local
[[ "$PWD" != *"remote root's" ]] || host=remote
if [[ "$FIXTURE_CASE" == interrupt ]]; then
  [[ $(cat "$FIXTURE_ROOT/batch-stopped") == stopped ]]
fi
printf '%s:%s:%s\n' "$host" "$1" "${2:-}" >> "$FIXTURE_ROOT/actions"
[[ "$1" == finalize && $(cat .tmp/e2e/run/retention-run-id 2>/dev/null) == "$2" ]] || exit 1
[[ "$FIXTURE_CASE" != refusal || "$host:$1" != local:finalize ]] || exit 23
printf '%s:finished:%s\n' "$host" "$3" >> "$FIXTURE_ROOT/actions"
MOCK
cp "$fixture/local/scripts/harness/lan/drv.sh" "$fixture/remote root's/scripts/harness/lan/drv.sh"
chmod +x "$fixture/bin/"*
export PATH="$fixture/bin:$PATH"
unset BASH_ENV AURA_E2E_REMOTE
export IN_NIX_SHELL=fixture
for FIXTURE_CASE in success failure refusal foreign missing interrupt; do
  export FIXTURE_CASE
  token="lan-batch-fixture-$FIXTURE_CASE"
  for checkout in "$fixture/local" "$fixture/remote root's"; do
    mkdir -p "$checkout/.tmp/e2e/run"
    printf '%s\n' "$token" > "$checkout/.tmp/e2e/run/retention-run-id"
  done
  [[ "$FIXTURE_CASE" != foreign ]] || printf '%s\n' another-owned-run > "$fixture/local/.tmp/e2e/run/retention-run-id"
  [[ "$FIXTURE_CASE" != missing ]] || rm "$fixture/local/.tmp/e2e/run/retention-run-id"
  : > "$fixture/actions"
  status=0
  bash "$fixture/local/scripts/harness/lan/batch.sh" "$token" -- bash "$fixture/local/work/handoff/lan-checklist.sh" > "$fixture/output" 2>&1 || status=$?
  case "$FIXTURE_CASE" in
    success) [[ "$status" == 0 ]]; outcome=success;;
    failure) [[ "$status" == 7 ]]; outcome=failed;;
    refusal|foreign|missing) [[ "$status" == 1 ]]; outcome=success;;
    interrupt) [[ "$status" == 143 ]]; outcome=failed;;
  esac
  expected=$'local:finalize:'"$token"$'\nlocal:finished:'"$outcome"$'\nremote:finalize:'"$token"$'\nremote:finished:'"$outcome"
  [[ "$FIXTURE_CASE" != refusal ]] || expected=$'local:finalize:'"$token"$'\nremote:finalize:'"$token"$'\nremote:finished:success'
  [[ "$FIXTURE_CASE" != foreign && "$FIXTURE_CASE" != missing ]] || expected=$'local:finalize:'"$token"$'\nremote:finalize:'"$token"$'\nremote:finished:success'
  [[ $(cat "$fixture/actions") == "$expected" ]] || { cat "$fixture/output" "$fixture/actions"; exit 1; }
  [[ $(cat "$fixture/local/.tmp/lan-checklist-$token/exit-status") == "$status" ]]
done
echo 'LAN batch finalization fixtures passed'
