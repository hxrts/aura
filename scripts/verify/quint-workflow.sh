#!/usr/bin/env bash
# Run Quint workflows with explicit selected source scope.
set -euo pipefail

mode="${1:-check}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Both declared Quint roots participate; no basename or subsystem exemption.
collect_specs() {
  local root
  for root in verification/quint crates/aura-simulator/tests/quint_specs; do
    [[ -d "$root" ]] || { echo "required Quint source root missing: $root" >&2; return 1; }
    find "$root" -type f -name '*.qnt' -print || return
  done
}

case "$mode" in
  setup)
    echo "Verifying Quint setup"
    nix develop --command quint --version
    nix develop --command node --version
    nix develop --command java -version
    temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/aura-quint-setup.XXXXXX")"
    trap 'rm -rf -- "$temporary_dir"' EXIT
    printf 'module simple { val x = 1 }\n' > "$temporary_dir/simple.qnt"
    nix develop --command quint parse "$temporary_dir/simple.qnt" > /dev/null
    echo "[OK] Basic parsing works"
    echo "Quint setup verification completed!"
    ;;
  check)
    command -v quint >/dev/null 2>&1 || { echo "quint not found; run inside nix develop" >&2; exit 1; }
    inventory="$(collect_specs | LC_ALL=C sort)"
    [[ -n "$inventory" ]] || { echo "no declared Quint sources found" >&2; exit 1; }
    passed=0
    failed=0
    echo "Typechecking declared Quint source inventory (both roots, recursive)"
    while IFS= read -r spec; do
      if quint typecheck "$spec"; then
        printf '[OK] %s\n' "$spec"
        passed=$((passed + 1))
      else
        printf '[FAIL] %s\n' "$spec" >&2
        failed=$((failed + 1))
      fi
    done <<< "$inventory"
    printf 'Selected: %s, Passed: %s, Failed: %s\n' "$((passed + failed))" "$passed" "$failed"
    [[ "$failed" == 0 ]]
    ;;
  models)
    command -v quint >/dev/null 2>&1 || { echo "quint not found; run inside nix develop" >&2; exit 1; }
    cd verification/quint
    # Preserve the existing inexpensive root-only typecheck selection and the
    # three actual bounded model checks. This is not a whole-inventory proof.
    selected=0
    for spec in *.qnt; do
      [[ -f "$spec" ]] || continue
      printf 'Typechecking selected top-level spec: %s\n' "$spec"
      quint typecheck "$spec"
      selected=$((selected + 1))
    done
    [[ "$selected" -gt 0 ]] || { echo "no top-level Quint sources found" >&2; exit 1; }
    printf 'Selected top-level typechecks passed: %s (nested sources not covered by this selection)\n' "$selected"
    quint verify --invariant=AllInvariants consensus/core.qnt --max-steps=10
    quint verify --invariant=InvariantByzantineThreshold consensus/adversary.qnt --max-steps=10
    quint verify --invariant=InvariantProgressUnderSynchrony consensus/liveness.qnt --max-steps=10
    echo "[OK] Three selected bounded consensus invariant checks passed; other model-check status unknown"
    ;;
  *) echo "Valid modes: setup, check, models" >&2; exit 2 ;;
esac
