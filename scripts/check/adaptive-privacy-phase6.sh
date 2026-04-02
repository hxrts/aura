#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

artifact_root="${AURA_ADAPTIVE_PRIVACY_ARTIFACT_ROOT:-$repo_root/artifacts/adaptive-privacy/phase6}"
rm -rf "$artifact_root"
mkdir -p "$artifact_root"
export AURA_ADAPTIVE_PRIVACY_ARTIFACT_ROOT="$artifact_root"

cargo test -p aura-simulator --test adaptive_privacy_phase_six -- --nocapture

test -f "$artifact_root/tuning_report.json"
test -f "$artifact_root/matrix_results.json"
test -f "$artifact_root/control-plane/index.json"
test -f "$artifact_root/parity/report.json"

echo "adaptive-privacy-phase6: archived artifacts at $artifact_root"
