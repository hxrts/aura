#!/usr/bin/env bash
# Raw-SSH driver bootstrap and fail-closed dependency prerequisites.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
fixture_root=$(mktemp -d)
trap 'rm -rf "$fixture_root"' EXIT
export AURA_E2E_ROOT="$fixture_root/checkout root's"
export AURA_E2E_RUN_DIR="$AURA_E2E_ROOT/retained state"
export AURA_E2E_HOST_ADDR=127.0.0.1 AURA_E2E_RUN_TOKEN=driver-environment-test
export FIXTURE_NIX_ARGS="$fixture_root/nix-args"
export AURA_NIX_BIN="$fixture_root/nix"
unset IN_NIX_SHELL BASH_ENV
mkdir -p "$AURA_E2E_ROOT" "$fixture_root/without-jq"
cat > "$AURA_NIX_BIN" <<'NIX'
#!/bin/bash
set -euo pipefail
printf '%s\n' "$@" > "$FIXTURE_NIX_ARGS"
exit 77
NIX
chmod +x "$AURA_NIX_BIN"
# Every entry point must bootstrap with byte-preserved arguments before any
# config, identity, process, FIFO or evidence action is attempted.
for command in start req stop finish; do
  case "$command" in
    start) args=(start "config path's.toml");;
    req) args=(req '{"method":"ui_state","params":{"instance_id":"actor name"}}' 7);;
    stop) args=(stop);;
    finish) args=(finish failed);;
  esac
  status=0
  bash "$here/drv.sh" "${args[@]}" >/dev/null 2> "$fixture_root/error" || status=$?
  [[ "$status" == 77 ]]
  printf '%s\n' develop "$AURA_E2E_ROOT" --command bash "$here/drv.sh" "${args[@]}" > "$fixture_root/expected"
  cmp "$fixture_root/expected" "$FIXTURE_NIX_ARGS"
  [[ ! -e "$AURA_E2E_RUN_DIR" ]]
done
# Entered shells lacking jq must fail before making run directories or
# weakening process identity verification, for all command entry points.
ln -s /bin/bash "$fixture_root/without-jq/bash"
ln -s /usr/bin/dirname "$fixture_root/without-jq/dirname"
original_path=$PATH
for command in start req stop finish; do
  status=0
  IN_NIX_SHELL=fixture PATH="$fixture_root/without-jq" /bin/bash "$here/drv.sh" "$command" unused > "$fixture_root/out" 2> "$fixture_root/error" || status=$?
  [[ "$status" == 127 ]]
  PATH="$original_path" rg -q 'requires jq in the pinned Nix environment' "$fixture_root/error"
  [[ ! -e "$AURA_E2E_RUN_DIR" ]]
done
status=0
AURA_NIX_BIN="$fixture_root/no-nix" bash "$here/drv.sh" stop > "$fixture_root/out" 2> "$fixture_root/error" || status=$?
[[ "$status" == 127 && ! -e "$AURA_E2E_RUN_DIR" ]]
echo 'LAN driver environment fixtures passed'
