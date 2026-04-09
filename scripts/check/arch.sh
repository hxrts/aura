#!/usr/bin/env bash
# Aura architectural compliance checker (orchestrator).
#
# Dispatches to arch-*.sh sub-scripts. Run with --help for options.
# Individual scripts can also be run directly.

source "$(dirname "$0")/arch-lib.sh"

usage() {
  cat <<'EOF'
Aura Architectural Compliance Checker

Usage: scripts/check/arch.sh [OPTIONS]

Options (run all when none given):
  --layers         Layer purity and dependency direction
  --deps           (alias for --layers)
  --effects        Effect system governance, crypto, concurrency
  --invariants     ARCHITECTURE.md invariant section validation
  --reactive       TUI reactive data model
  --ceremonies     Ceremony fact-commit validation
  --ui             UI boundary checks
  --workflows      aura-app workflow hygiene
  --serialization  Wire-format and handler hygiene
  --style          Repo hygiene (lonely mod.rs, empty dirs)
  --test-seeds     Test seed uniqueness
  --todos          TODOs, placeholders, incomplete markers
  --quick          Skip slow checks (todos, placeholders)
  --layer N[,M...] Filter to specific layers (1-8)
  -v, --verbose    Show more detail
  -h, --help       Show this help
EOF
}

RUN_ALL=true RUN_QUICK=false
RUN_LAYERS=false RUN_EFFECTS=false RUN_INVARIANTS=false
RUN_REACTIVE=false RUN_CEREMONIES=false RUN_UI=false
RUN_WORKFLOWS=false RUN_SERIALIZATION=false RUN_STYLE=false
RUN_TEST_SEEDS=false RUN_TODOS=false

while [[ $# -gt 0 ]]; do
  case $1 in
    --layers|--deps) RUN_ALL=false; RUN_LAYERS=true ;;
    --effects|--crypto|--concurrency) RUN_ALL=false; RUN_EFFECTS=true ;;
    --invariants)    RUN_ALL=false; RUN_INVARIANTS=true ;;
    --reactive)      RUN_ALL=false; RUN_REACTIVE=true ;;
    --ceremonies)    RUN_ALL=false; RUN_CEREMONIES=true ;;
    --ui)            RUN_ALL=false; RUN_UI=true ;;
    --workflows)     RUN_ALL=false; RUN_WORKFLOWS=true ;;
    --serialization) RUN_ALL=false; RUN_SERIALIZATION=true ;;
    --style)         RUN_ALL=false; RUN_STYLE=true ;;
    --test-seeds)    RUN_ALL=false; RUN_TEST_SEEDS=true ;;
    --todos)         RUN_ALL=false; RUN_TODOS=true ;;
    --layer)
      [[ -z "${2-}" ]] && { echo "--layer requires N"; exit 1; }
      IFS=',' read -ra layers <<< "$2"
      LAYER_FILTERS+=("${layers[@]}")
      shift ;;
    --quick)      RUN_QUICK=true ;;
    -v|--verbose) VERBOSE=true ;;
    -h|--help)    usage; exit 0 ;;
    *)            echo "Unknown: $1"; usage; exit 1 ;;
  esac
  shift
done

if $RUN_QUICK && $RUN_ALL; then
  RUN_ALL=false
  RUN_LAYERS=true RUN_EFFECTS=true RUN_INVARIANTS=true
  RUN_REACTIVE=true RUN_CEREMONIES=true RUN_SERIALIZATION=true
  RUN_STYLE=true RUN_WORKFLOWS=true RUN_TEST_SEEDS=true
  RUN_TODOS=false
fi

export VERBOSE LAYER_FILTERS

DIR="$(dirname "$0")"

run() {
  local enabled="$1" script="$2"
  if $RUN_ALL || $enabled; then
    source "$DIR/$script"
  fi
}

run "$RUN_LAYERS"        arch-layer-deps.sh
run "$RUN_EFFECTS"       arch-effects.sh
run "$RUN_INVARIANTS"    arch-invariants.sh
run "$RUN_REACTIVE"      arch-reactive.sh
run "$RUN_CEREMONIES"    arch-ceremonies.sh
run "$RUN_UI"            arch-ui.sh
run "$RUN_WORKFLOWS"     arch-workflows.sh
run "$RUN_SERIALIZATION" arch-serialization.sh
run "$RUN_STYLE"         arch-style.sh
if $RUN_ALL || $RUN_TEST_SEEDS; then
  if ! AURA_CHECK_ARCH_MODE=1 bash "$DIR/testing-seed-uniqueness.sh"; then
    violation "Test seed uniqueness violations detected"
  fi
fi
run "$RUN_TODOS"         arch-todos.sh

arch_summary

if [[ $VIOLATIONS -gt 10 ]] && ! $RUN_QUICK; then
  printc "\n${YELLOW}Tip:${NC} Use --quick to skip TODO/placeholder checks"
fi

[[ $VIOLATIONS -eq 0 ]]
