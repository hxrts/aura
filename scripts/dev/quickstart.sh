#!/usr/bin/env bash
set -euo pipefail

action="${1:-init}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

case "$action" in
  init)
    cargo run --bin aura -- init -n 3 -t 2 -o .aura
    ;;
  status)
    cargo run --bin aura -- status -c .aura/configs/device_1.toml
    ;;
  smoke)
    echo "Running quickstart smoke checks"
    echo "============================="
    rm -rf .aura-test

    echo "1. Initializing 2-of-3 threshold account..."
    init_log="$(mktemp)"
    cargo run --bin aura -- init -n 3 -t 2 -o .aura-test | tee "$init_log"
    echo "OK Account initialized"

    echo "2. Verifying effect_api and config entries..."
    grep -q "Created effect API metadata" "$init_log" && echo "OK Effect API metadata created" || {
      echo "ERROR: Effect API metadata missing"
      exit 1
    }
    grep -q "Created device_1.toml" "$init_log" && echo "OK Config entry created" || {
      echo "ERROR: Config entry missing"
      exit 1
    }
    rm -f "$init_log"

    echo "3. Checking account status..."
    status_out="$(cargo run --bin aura -- status -c .aura-test/configs/device_1.toml)"
    echo "$status_out"
    echo "$status_out" | grep -q "Configuration loaded successfully" || {
      echo "ERROR: Config not found in storage"
      exit 1
    }
    echo "OK Status retrieved"

    echo "4. Testing multi-device configs..."
    for i in 1 2 3; do
      device_out="$(cargo run --bin aura -- status -c .aura-test/configs/device_${i}.toml)"
      echo "$device_out" | grep -q "Configuration loaded successfully" && echo "   [OK] Device ${i} config found" || {
        echo "ERROR"
        exit 1
      }
    done

    echo "5. Testing threshold signature operation..."
    cargo run --bin aura -- threshold \
      --configs .aura-test/configs/device_1.toml,.aura-test/configs/device_2.toml \
      --threshold 2 --mode sign > /dev/null 2>&1 && echo "OK Threshold signature passed" || {
      echo "FAIL"
      exit 1
    }

    echo
    echo "Quickstart smoke checks passed!"
    ;;
  *)
    echo "Unknown quickstart action: $action"
    echo "Valid actions: init, status, smoke"
    exit 2
    ;;
esac
