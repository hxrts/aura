#!/usr/bin/env bash
set -euo pipefail

mode="${1:-check}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

case "$mode" in
  setup)
    echo "Verifying Quint setup"
    echo "====================="
    nix develop --command quint --version
    nix develop --command node --version
    nix develop --command java -version
    echo 'module simple { val x = 1 }' > /tmp/simple.qnt
    nix develop --command quint parse /tmp/simple.qnt > /dev/null && echo "[OK] Basic parsing works"
    echo "Quint setup verification completed!"
    ;;
  check)
    if ! command -v quint >/dev/null 2>&1; then
      echo "quint not found in PATH; run inside 'nix develop' or use 'just quint setup' first"
      exit 1
    fi

    GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
    echo "Typechecking all Quint specs"
    echo "============================"
    passed=0
    failed=0

    for dir in "verification/quint" "verification/quint/consensus" "verification/quint/journal" \
               "verification/quint/keys" "verification/quint/sessions" "crates/aura-simulator/tests/quint_specs"; do
      [ -d "$dir" ] || continue
      for spec in "$dir"/*.qnt; do
        [ -f "$spec" ] || continue
        if quint typecheck "$spec" > /dev/null 2>&1; then
          echo -e "  ${GREEN}✓${NC} $(basename "$spec")"
          ((passed++))
        else
          echo -e "  ${RED}✗${NC} $(basename "$spec")"
          ((failed++))
        fi
      done
    done

    echo
    echo "Passed: $passed, Failed: $failed"
    [ "$failed" -gt 0 ] && exit 1 || echo -e "${GREEN}All specs passed!${NC}"
    ;;
  models)
    if ! command -v quint >/dev/null 2>&1; then
      echo "quint not found in PATH; run inside 'nix develop'"
      exit 1
    fi

    GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'
    echo "Quint model checking"
    echo "===================="
    cd verification/quint

    echo "[1/2] Typechecking all Quint specifications..."
    for spec in *.qnt; do
      echo "  Checking $spec..."
      quint typecheck "$spec" || { echo -e "${RED}[FAIL]${NC} $spec"; exit 1; }
    done
    echo -e "${GREEN}[OK]${NC} All specs typecheck"

    echo "[2/2] Running Quint invariant verification..."
    quint verify --invariant=AllInvariants consensus/core.qnt --max-steps=10 || { echo -e "${RED}[FAIL]${NC} consensus/core.qnt"; exit 1; }
    quint verify --invariant=InvariantByzantineThreshold consensus/adversary.qnt --max-steps=10 || { echo -e "${RED}[FAIL]${NC} consensus/adversary.qnt"; exit 1; }
    quint verify --invariant=InvariantProgressUnderSynchrony consensus/liveness.qnt --max-steps=10 || { echo -e "${RED}[FAIL]${NC} consensus/liveness.qnt"; exit 1; }

    echo -e "${GREEN}[OK]${NC} All invariants pass"
    ;;
  *)
    echo "Unknown quint mode: $mode"
    echo "Valid modes: setup, check, models"
    exit 2
    ;;
esac
