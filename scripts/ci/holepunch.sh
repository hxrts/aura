#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
if [[ -z "$mode" ]]; then
  echo "usage: scripts/ci/holepunch.sh <tier2|daily|nightly|verify-artifacts|triage|audit>"
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

prepare() {
  local artifact_dir="$1"
  mkdir -p "$artifact_dir"
  mkdir -p artifacts/patchbay/work target/patchbay-vm
}

run_test() {
  local artifact_dir="$1"
  shift
  AURA_HOLEPUNCH_ARTIFACT_DIR="${PWD}/${artifact_dir}" \
    QEMU_VM_WORK_DIR="${PWD}/artifacts/patchbay/work" \
    CARGO_TARGET_DIR="${PWD}/target/patchbay-vm" \
    NETSIM_TARGET_DIR="${PWD}/target/patchbay-vm" \
    cargo test -p aura-harness "$@" -q
}

case "$mode" in
  tier2)
    prepare artifacts/holepunch/tier2
    run_test artifacts/holepunch/tier2 --test holepunch_tier2_patchbay --test holepunch_e2e_runtime_patchbay
    ;;
  daily)
    prepare artifacts/holepunch/daily
    runs="${AURA_HOLEPUNCH_DAILY_RUNS:-5}"
    failures=0
    for run in $(seq 1 "$runs"); do
      echo "daily-smoke run $run/$runs"
      if ! run_test "artifacts/holepunch/daily/run-${run}" --test holepunch_tier2_patchbay; then
        failures=$((failures + 1))
      fi
    done

    flake_rate=$(awk -v f="$failures" -v r="$runs" 'BEGIN { if (r == 0) { print 0 } else { printf "%.4f", (f / r) } }')
    cat > artifacts/holepunch/daily/flake-rate.json <<JSON
{
  "runs": $runs,
  "failures": $failures,
  "flake_rate": $flake_rate
}
JSON
    ;;
  nightly)
    prepare artifacts/holepunch/nightly
    run_test artifacts/holepunch/nightly --test holepunch_tier3_stress
    ;;
  verify-artifacts)
    test -d artifacts/holepunch/nightly
    test -n "$(find artifacts/holepunch/nightly -type f | head -n 1)"
    ;;
  triage)
    prepare artifacts/holepunch/weekly
    runs="${AURA_HOLEPUNCH_TRIAGE_RUNS:-10}"
    failures=0
    for run in $(seq 1 "$runs"); do
      if ! run_test "artifacts/holepunch/weekly/triage-run-${run}" --test holepunch_tier2_patchbay; then
        failures=$((failures + 1))
      fi
    done

    cat > artifacts/holepunch/weekly/flaky-triage.md <<EOF_MD
# Holepunch Flaky Test Triage
- total_runs: $runs
- failed_runs: $failures
- action: quarantine unstable cases when failed_runs > 0 and open stabilization issues
EOF_MD
    ;;
  audit)
    mkdir -p artifacts/holepunch/weekly
    rg -n -F 'name = "patchbay"' Cargo.lock
    rg -n -F 'source = "git+https://github.com/hxrts/patchbay?branch=hxrts/aura#' Cargo.lock
    {
      echo "# Holepunch Toolchain Audit"
      echo "cargo-lock-pin: ok"
      if command -v patchbay-vm >/dev/null 2>&1; then
        echo "patchbay-vm: $(patchbay-vm --version 2>/dev/null || echo installed)"
      else
        echo "patchbay-vm: not installed in runner"
      fi
    } > artifacts/holepunch/weekly/toolchain-audit.md
    ;;
  *)
    echo "Unknown mode: $mode"
    echo "Valid modes: tier2, daily, nightly, verify-artifacts, triage, audit"
    exit 2
    ;;
esac
