# Justfile for Aura project automation
#
# Run `just` or `just --list` to see available commands

# ═══════════════════════════════════════════════════════════════════════════════
# Configuration
# ═══════════════════════════════════════════════════════════════════════════════

# Expected CI Rust version (update when GitHub CI updates)
CI_RUST_VERSION := "1.92"

# Default recipe - show available commands
default:
    @just --list

# ═══════════════════════════════════════════════════════════════════════════════
# Build
# ═══════════════════════════════════════════════════════════════════════════════

# Build all crates
build:
    cargo build --workspace -q

# Build in release mode
build-release:
    cargo build --workspace --release -q

# Build Aura terminal in development mode (release profile with dev features)
build-dev:
    cargo build -p aura-terminal --bin aura --features development --release
    mkdir -p bin
    cp target/release/aura bin/aura
    @echo "Binary available at: ./bin/aura"

# Build terminal in release mode without dev features
build-terminal-release:
    cargo build -p aura-terminal --bin aura --release --no-default-features --features terminal
    mkdir -p bin
    cp target/release/aura bin/aura
    @echo "Binary available at: ./bin/aura"

# Build app-host binary
build-app-host:
    cargo build -p aura-app --bin app-host --features host --release

# ═══════════════════════════════════════════════════════════════════════════════
# Test
# ═══════════════════════════════════════════════════════════════════════════════

# Run all tests
test:
    cargo test --workspace -q

# Run all tests with output
test-verbose:
    cargo test --workspace --verbose -- --nocapture

# Run tests for a specific crate
test-crate crate:
    cargo test -p {{crate}} -q

# Run tests for a specific crate in isolation (lib + unit tests only)
test-crate-isolated crate:
    #!/usr/bin/env bash
    echo "Testing {{crate}} in isolation (lib + unit tests only)..."
    cd "crates/{{crate}}" && cargo test --lib --verbose

# ═══════════════════════════════════════════════════════════════════════════════
# Linting & Formatting
# ═══════════════════════════════════════════════════════════════════════════════

# Check code without building
check:
    cargo check --workspace -q

# Run the exact same check that Zed editor runs (rust-analyzer checkOnSave)
check-zed:
    cargo check --workspace --all-targets

# Run clippy linter
clippy:
    cargo clippy --workspace --all-targets -q -- -D warnings

# Strict clippy check enforcing effects system usage
clippy-strict:
    cargo clippy --workspace --all-targets -q -- \
        -D warnings \
        -D clippy::disallowed_methods \
        -D clippy::disallowed_types

# Ensure Layer 4 crates do not have crate-level allow attributes
check-layer4-lints:
    #!/usr/bin/env bash
    set -euo pipefail
    L4_CRATES="crates/aura-guards/src/lib.rs crates/aura-consensus/src/lib.rs"
    L4_CRATES="$L4_CRATES crates/aura-amp/src/lib.rs crates/aura-anti-entropy/src/lib.rs"
    L4_CRATES="$L4_CRATES crates/aura-protocol/src/lib.rs"
    if rg -n "^#!\[allow" $L4_CRATES; then
        echo "crate-level #![allow] found in Layer 4 lib.rs; move to module scope"
        exit 1
    fi

# Format code
fmt:
    cargo fmt --all

# Check code formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run security audit
audit:
    cargo audit

# ═══════════════════════════════════════════════════════════════════════════════
# Architecture Checks
# ═══════════════════════════════════════════════════════════════════════════════

# Check architectural layer compliance (all checks)
check-arch *FLAGS:
    scripts/check-arch.sh {{FLAGS}} || true

# Quick architecture checks by category
check-arch-layers:
    scripts/check-arch.sh --layers || true

check-arch-effects:
    scripts/check-arch.sh --effects || true

check-arch-deps:
    scripts/check-arch.sh --deps || true

check-arch-completeness:
    scripts/check-arch.sh --completeness || true

check-arch-todos:
    scripts/check-arch.sh --todos || true

check-arch-concurrency:
    scripts/check-arch.sh --concurrency || true

# ═══════════════════════════════════════════════════════════════════════════════
# CI Steps (called by ci-dry-run and GitHub workflows)
# ═══════════════════════════════════════════════════════════════════════════════

# Build documentation book
ci-book: summary
    echo '.chapter-item a strong { display: none; }' > custom.css
    AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command bash -c \
        'mdbook-mermaid install . > /dev/null 2>&1 || true && mdbook build && rm -f mermaid-init.js mermaid.min.js custom.css'

# Format check
ci-format:
    cargo fmt --all -- --check

# Clippy with effects enforcement (matches CI environment)
# Note: CARGO_INCREMENTAL=0 forces fresh lint checking
# Note: Nix clippy may not catch all implicit_clone cases; GitHub CI uses newer clippy
ci-clippy:
    CARGO_INCREMENTAL=0 RUSTFLAGS="-D warnings" cargo clippy --workspace --all-targets -- \
        -D warnings \
        -D clippy::disallowed_methods \
        -D clippy::disallowed_types \
        -D clippy::unwrap_used \
        -D clippy::expect_used \
        -D clippy::duplicated_attributes \
        -D clippy::implicit_clone

# Build check
ci-build:
    cargo build --workspace -q

# Test suite
ci-test:
    cargo test --workspace -q

# Effects system violation checks
ci-effects:
    #!/usr/bin/env bash
    set -euo pipefail
    EXCLUDE="--glob '!**/aura-effects/**' --glob '!**/aura-agent/**' --glob '!**/aura-simulator/**'"
    EXCLUDE="$EXCLUDE --glob '!**/aura-terminal/**' --glob '!**/aura-testkit/**' --glob '!**/tests/**'"
    EXCLUDE="$EXCLUDE --glob '!**/integration/**' --glob '!**/demo/**' --glob '!**/examples/**'"

    # Check time usage
    if rg --type rust "SystemTime::now|Instant::now|chrono::Utc::now" crates/ $EXCLUDE 2>/dev/null; then
        echo "::error::Found direct time usage! Use TimeEffects instead."
        exit 1
    fi
    # Check randomness usage
    if rg --type rust "rand::random|thread_rng\(\)|OsRng::new" crates/ $EXCLUDE 2>/dev/null; then
        echo "::error::Found direct randomness usage! Use RandomEffects instead."
        exit 1
    fi
    echo "No effects violations found"

# Quint typecheck
ci-quint-typecheck:
    #!/usr/bin/env bash
    set -euo pipefail
    cd verification/quint
    for spec in *.qnt consensus/*.qnt journal/*.qnt keys/*.qnt sessions/*.qnt; do
        [ -f "$spec" ] || continue
        echo "Checking $spec..."
        quint typecheck "$spec" || exit 1
    done
    echo "All Quint specs typecheck"

# Quint model checking
ci-quint-verify:
    #!/usr/bin/env bash
    set -euo pipefail
    cd verification/quint
    echo "Verifying consensus/core.qnt..."
    quint verify --invariant=AllInvariants consensus/core.qnt --max-steps=10
    echo "Verifying consensus/adversary.qnt..."
    quint verify --invariant=InvariantByzantineThreshold consensus/adversary.qnt --max-steps=10
    echo "Verifying consensus/liveness.qnt..."
    quint verify --invariant=InvariantProgressUnderSynchrony consensus/liveness.qnt --max-steps=10
    echo "All Quint invariants verified"

# Lean build
ci-lean-build:
    cd verification/lean && lake build

# Lean sorry check
ci-lean-check-sorry:
    #!/usr/bin/env bash
    set -euo pipefail
    if grep -r "sorry" verification/lean/Aura --include="*.lean" 2>/dev/null; then
        echo "::warning::Found incomplete proofs (sorry)"
    else
        echo "All proofs complete"
    fi

# Kani bounded model checking
ci-kani:
    cargo kani --package aura-protocol --default-unwind 10 --output-format terse

# ITF conformance tests
ci-conformance-itf:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p traces
    echo "Generating ITF traces..."
    quint run --out-itf=traces/consensus.itf.json verification/quint/consensus/core.qnt --max-steps=30 --max-samples=5
    echo "Running ITF conformance tests..."
    cargo test -p aura-testkit --test consensus_itf_conformance -- --nocapture

# Differential tests
ci-conformance-diff:
    cargo test -p aura-testkit --test consensus_differential -- --nocapture

# ═══════════════════════════════════════════════════════════════════════════════
# CI Dry Run
# ═══════════════════════════════════════════════════════════════════════════════

# Run CI checks locally (same recipes as GitHub CI)
ci-dry-run:
    #!/usr/bin/env bash
    set -euo pipefail
    GREEN='\033[0;32m' RED='\033[0;31m' YELLOW='\033[0;33m' BLUE='\033[0;34m' NC='\033[0m'
    exit_code=0

    run_step() {
        local num="$1" name="$2" cmd="$3"
        printf "[$num] $name... "
        if $cmd >/dev/null 2>&1; then
            echo -e "${GREEN}OK${NC}"
        else
            echo -e "${RED}FAIL${NC}"
            exit_code=1
        fi
    }

    echo "CI Dry Run"
    echo "=========="
    echo ""

    # Environment check
    LOCAL_RUST=$(rustc --version | grep -oE '[0-9]+\.[0-9]+' | head -1)
    printf "[0/7] Rust version... "
    if [[ "$LOCAL_RUST" == "{{CI_RUST_VERSION}}" ]]; then
        echo -e "${GREEN}$LOCAL_RUST${NC} (matches CI)"
    elif [[ "$LOCAL_RUST" < "{{CI_RUST_VERSION}}" ]]; then
        echo -e "${YELLOW}$LOCAL_RUST${NC} (CI uses {{CI_RUST_VERSION}} - newer lints may fail in CI)"
    else
        echo -e "${BLUE}$LOCAL_RUST${NC} (newer than CI {{CI_RUST_VERSION}})"
    fi
    echo ""

    # Run CI steps (same as GitHub workflows)
    run_step "1/7" "Format"  "just ci-format"
    run_step "2/7" "Clippy"  "just ci-clippy"
    run_step "3/7" "Build"   "just ci-build"
    run_step "4/7" "Test"    "just ci-test"
    run_step "5/7" "Effects" "just ci-effects"

    # Quint (optional - skip if not installed)
    printf "[6/7] Quint... "
    if command -v quint &>/dev/null; then
        if just ci-quint-typecheck >/dev/null 2>&1; then
            echo -e "${GREEN}OK${NC}"
        else
            echo -e "${RED}FAIL${NC}"
            exit_code=1
        fi
    else
        echo -e "${YELLOW}SKIP${NC} (quint not installed)"
    fi

    # Architecture check (warning only)
    printf "[7/7] Architecture... "
    if ./scripts/check-arch.sh --quick >/dev/null 2>&1; then
        echo -e "${GREEN}OK${NC}"
    else
        echo -e "${YELLOW}WARN${NC} (run 'just check-arch' for details)"
    fi

    # Summary
    echo ""
    if [ $exit_code -eq 0 ]; then
        echo -e "${GREEN}All CI checks passed${NC}"
    else
        echo -e "${RED}Some CI checks failed${NC}"
        exit 1
    fi

# ═══════════════════════════════════════════════════════════════════════════════
# Clean
# ═══════════════════════════════════════════════════════════════════════════════

# Clean build artifacts and generated files
clean:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Cleaning build artifacts..."
    cargo clean
    echo "Cleaning Nix outputs..."
    rm -rf result result-*
    echo "Cleaning documentation builds..."
    rm -rf docs/book/ mermaid.min.js mermaid-init.js custom.css
    echo "Cleaning logs..."
    rm -rf logs/ *.log
    echo "Cleaning demo/test data..."
    rm -rf .aura-demo/ .aura-test/ outcomes/ *.sealed *.dat *.tmp *.temp
    echo "Cleaning verification artifacts..."
    rm -rf verification/lean/.lake/ verification/lean/build/ verification/lean/Generated/
    rm -rf verification/lean/.lean_build/ verification/traces/ _apalache-out/
    echo "✓ Clean complete"

# Clean everything including production data (use with caution!)
clean-all: clean
    #!/usr/bin/env bash
    set -euo pipefail
    echo ""
    echo "WARNING: This will delete production data in .aura/"
    read -p "Are you sure? [y/N] " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        rm -rf .aura/ secure_store/
        echo "✓ All data cleaned"
    else
        echo "Aborted."
    fi

# ═══════════════════════════════════════════════════════════════════════════════
# Watch
# ═══════════════════════════════════════════════════════════════════════════════

# Watch and rebuild on changes
watch:
    cargo watch -x build

# Watch and run tests on changes
watch-test:
    cargo watch -x test

# ═══════════════════════════════════════════════════════════════════════════════
# Documentation
# ═══════════════════════════════════════════════════════════════════════════════

# Generate docs/SUMMARY.md from Markdown files
summary:
    #!/usr/bin/env bash
    set -euo pipefail
    docs="docs"; build_dir="$docs/book"; out="$docs/SUMMARY.md"

    get_title() {
        local title="$(grep -m1 '^# ' "$1" | sed 's/^# *//')"
        if [ -z "$title" ]; then
            title="$(basename "${1%.*}" | tr '._-' '   ' | awk '{for(i=1;i<=NF;i++){$i=toupper(substr($i,1,1))substr($i,2)}}1')"
        fi
        echo "$title"
    }

    echo "# Summary" > "$out"; echo "" >> "$out"

    declare -A dirs; declare -a root_files
    while IFS= read -r f; do
        rel="${f#$docs/}"
        [ "$rel" = "SUMMARY.md" ] && continue
        case "$f" in "$build_dir"/*) continue ;; esac
        if [[ "$rel" == */* ]]; then
            dir="${rel%%/*}"; dirs[$dir]+="$f"$'\n'
        else
            root_files+=("$f")
        fi
    done < <(find "$docs" -type f -name '*.md' -not -name 'SUMMARY.md' -not -path "$build_dir/*" | LC_ALL=C sort)

    for f in "${root_files[@]}"; do
        echo "- [$(get_title "$f")](${f#$docs/})" >> "$out"
    done

    for dir in $(printf '%s\n' "${!dirs[@]}" | LC_ALL=C sort); do
        [ ${#root_files[@]} -gt 0 ] && echo "" >> "$out"
        echo "# $(echo "$dir" | tr '_-' '  ' | awk '{for(i=1;i<=NF;i++){$i=toupper(substr($i,1,1))substr($i,2)}}1')" >> "$out"
        echo "" >> "$out"
        while IFS= read -r f; do
            [ -z "$f" ] && continue
            echo "- [$(get_title "$f")](${f#$docs/})" >> "$out"
        done < <(echo -n "${dirs[$dir]}" | LC_ALL=C sort)
    done
    echo "Wrote $out"

# Build the book after regenerating the summary
book: summary
    echo '.chapter-item a strong { display: none; }' > custom.css
    AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command bash -c \
        'mdbook-mermaid install . > /dev/null 2>&1 || true && mdbook build && rm -f mermaid-init.js mermaid.min.js custom.css'

# Serve locally with live reload
serve-book: summary
    #!/usr/bin/env bash
    set -euo pipefail
    pgrep -x mdbook > /dev/null && { echo "Stopping existing mdbook server..."; pkill mdbook; sleep 1; }
    echo '.chapter-item a strong { display: none; }' > custom.css
    trap 'rm -f mermaid-init.js mermaid.min.js custom.css' EXIT
    AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command bash -c \
        'mdbook-mermaid install . > /dev/null 2>&1 || true && mdbook serve --open'

# Serve documentation with live reload (alias)
serve: serve-book

# Generate rustdoc documentation
docs:
    cargo doc --workspace --no-deps --open

# ═══════════════════════════════════════════════════════════════════════════════
# Phase 0 / Smoke Tests
# ═══════════════════════════════════════════════════════════════════════════════

# Initialize a new account (Phase 0 smoke test)
init-account:
    cargo run --bin aura -- init -n 3 -t 2 -o .aura

# Show account status
status:
    cargo run --bin aura -- status -c .aura/configs/device_1.toml

# Run Phase 0 smoke tests
smoke-test:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Running Phase 0 Smoke Tests"
    echo "==========================="
    rm -rf .aura-test

    echo "1. Initializing 2-of-3 threshold account..."
    cargo run --bin aura -- init -n 3 -t 2 -o .aura-test
    echo "OK Account initialized"

    echo "2. Verifying effect_api and config files..."
    [ -f ".aura-test/effect_api.cbor" ] && echo "OK Effect API file created" || { echo "ERROR: Ledger file not found"; exit 1; }
    [ -f ".aura-test/configs/device_1.toml" ] && echo "OK Config file created" || { echo "ERROR: Config file not found"; exit 1; }

    echo "3. Checking account status..."
    cargo run --bin aura -- status -c .aura-test/configs/device_1.toml
    echo "OK Status retrieved"

    echo "4. Testing multi-device configs..."
    for i in 1 2 3; do
        [ -f ".aura-test/configs/device_${i}.toml" ] && echo "   [OK] Device ${i} config found" || { echo "ERROR"; exit 1; }
    done

    echo "5. Testing threshold signature operation..."
    cargo run --bin aura -- threshold \
        --configs .aura-test/configs/device_1.toml,.aura-test/configs/device_2.toml \
        --threshold 2 --mode local > /dev/null 2>&1 && echo "OK Threshold signature passed" || { echo "FAIL"; exit 1; }

    echo ""
    echo "Phase 0 smoke tests passed!"

# Run macOS Keychain integration tests
test-macos-keychain:
    #!/usr/bin/env bash
    set -e
    [[ "$OSTYPE" != "darwin"* ]] && { echo "Error: macOS only"; exit 1; }
    echo "Aura macOS Keychain Integration Tests"
    echo "======================================"
    read -p "Continue with keychain tests? (y/N): " -n 1 -r; echo
    [[ ! $REPLY =~ ^[Yy]$ ]] && { echo "Cancelled"; exit 0; }
    export RUST_LOG=debug RUST_BACKTRACE=1
    cargo test -p aura-agent --test macos_keychain_tests -- --nocapture --test-threads=1

# ═══════════════════════════════════════════════════════════════════════════════
# Quint Specifications
# ═══════════════════════════════════════════════════════════════════════════════

# Verify Quint setup
verify-quint:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Verifying Quint Setup"
    echo "====================="
    nix develop --command quint --version
    nix develop --command node --version
    nix develop --command java -version
    echo 'module simple { val x = 1 }' > /tmp/simple.qnt
    nix develop --command quint parse /tmp/simple.qnt > /dev/null && echo "[OK] Basic parsing works"
    echo "Quint setup verification completed!"

# Parse Quint file to JSON
quint-parse input output:
    nix develop --command quint parse --out {{output}} {{input}}

# Parse and typecheck Quint file
quint-compile input output:
    nix develop --command quint typecheck {{input}}
    nix develop --command quint parse --out {{output}} {{input}}

# Typecheck all Quint specs (fast sanity check)
quint-typecheck-all:
    #!/usr/bin/env bash
    set -uo pipefail
    GREEN='\033[0;32m' RED='\033[0;31m' NC='\033[0m'
    echo "Typechecking All Quint Specs"
    echo "============================"
    passed=0; failed=0

    for dir in "verification/quint" "verification/quint/consensus" "verification/quint/journal" \
               "verification/quint/keys" "verification/quint/sessions" "crates/aura-simulator/tests/quint_specs"; do
        [ -d "$dir" ] || continue
        for spec in "$dir"/*.qnt; do
            [ -f "$spec" ] || continue
            if quint typecheck "$spec" > /dev/null 2>&1; then
                echo -e "  ${GREEN}✓${NC} $(basename "$spec")"; ((passed++))
            else
                echo -e "  ${RED}✗${NC} $(basename "$spec")"; ((failed++))
            fi
        done
    done

    echo ""; echo "Passed: $passed, Failed: $failed"
    [ $failed -gt 0 ] && exit 1 || echo -e "${GREEN}All specs passed!${NC}"

# Quint model checking with invariant verification (matches CI)
quint-verify-models:
    #!/usr/bin/env bash
    set -euo pipefail
    GREEN='\033[0;32m' RED='\033[0;31m' NC='\033[0m'

    echo "Quint Model Checking"
    echo "===================="
    cd verification/quint

    echo "[1/2] Typechecking all Quint specifications..."
    for spec in *.qnt; do
        echo "  Checking $spec..."
        quint typecheck "$spec" || { echo -e "${RED}[FAIL]${NC} $spec"; exit 1; }
    done
    echo -e "${GREEN}[OK]${NC} All specs typecheck"

    echo "[2/2] Running Quint invariant verification..."
    quint verify --invariant=AllInvariants consensus/core.qnt --max-steps=10 || \
        { echo -e "${RED}[FAIL]${NC} consensus/core.qnt"; exit 1; }
    quint verify --invariant=InvariantByzantineThreshold consensus/adversary.qnt --max-steps=10 || \
        { echo -e "${RED}[FAIL]${NC} consensus/adversary.qnt"; exit 1; }
    quint verify --invariant=InvariantProgressUnderSynchrony consensus/liveness.qnt --max-steps=10 || \
        { echo -e "${RED}[FAIL]${NC} consensus/liveness.qnt"; exit 1; }

    echo -e "${GREEN}[OK]${NC} All invariants pass"

# Verify a single Quint spec with specific invariants
quint-verify spec invariants:
    #!/usr/bin/env bash
    set -uo pipefail
    mkdir -p verification/quint/traces
    SPEC_NAME=$(basename "{{spec}}" .qnt)
    INV_FLAGS=""
    IFS=',' read -ra INVS <<< "{{invariants}}"
    for inv in "${INVS[@]}"; do INV_FLAGS="$INV_FLAGS --invariant=$inv"; done
    quint verify "{{spec}}" $INV_FLAGS --max-steps=10 \
        --out-itf="verification/quint/traces/${SPEC_NAME}_counter.itf.json" --verbosity=3

# Generate ITF traces from Quint specs
quint-generate-traces:
    #!/usr/bin/env bash
    set -uo pipefail
    GREEN='\033[0;32m' RED='\033[0;31m' NC='\033[0m'
    echo "Generating ITF Traces"
    echo "====================="
    mkdir -p verification/quint/traces

    SPECS=(
        "verification/quint/consensus/core.qnt:consensus"
        "verification/quint/journal/core.qnt:journal"
    )
    for target in "${SPECS[@]}"; do
        IFS=':' read -r spec name <<< "$target"
        [ -f "$spec" ] || continue
        if quint run "$spec" --max-steps=15 --out-itf="verification/quint/traces/${name}.itf.json" >/dev/null 2>&1; then
            echo -e "  ${GREEN}✓${NC} $name"
        else
            echo -e "  ${RED}✗${NC} $name"
        fi
    done

# Check Quint-Rust type correspondence
quint-check-types verbose="":
    ./scripts/verify.sh quint-types {{verbose}}

# Generate verification coverage report
verification-coverage format="--md":
    ./scripts/verify.sh coverage {{format}}

# ═══════════════════════════════════════════════════════════════════════════════
# TUI ITF Traces
# ═══════════════════════════════════════════════════════════════════════════════

# Regenerate deterministic ITF trace for TUI replay tests
tui-itf-trace out="verification/quint/traces/tui_trace.itf.json" seed="424242" max_steps="50":
    TUI_ITF_SEED={{seed}} TUI_ITF_MAX_STEPS={{max_steps}} nix develop --command ./scripts/tui-itf-trace.sh generate {{out}}

# Check that the checked-in ITF trace matches regeneration
tui-itf-trace-check seed="424242" max_steps="50":
    TUI_ITF_SEED={{seed}} TUI_ITF_MAX_STEPS={{max_steps}} nix develop --command ./scripts/tui-itf-trace.sh check

# ═══════════════════════════════════════════════════════════════════════════════
# Lean Formal Verification
# ═══════════════════════════════════════════════════════════════════════════════

# Initialize Lean project (run once or after clean)
lean-init:
    @echo "Initializing Lean project..."
    cd verification/lean && lake update

# Build Lean verification modules
lean-build jobs="2": lean-init
    @echo "Building Lean verification modules (threads={{jobs}})..."
    cd verification/lean && nice -n 15 lake build -K env.LEAN_THREADS={{jobs}}

# Build the Lean oracle verifier CLI for differential testing
lean-oracle-build: lean-init
    #!/usr/bin/env bash
    set -euo pipefail
    PROJECT_ROOT="$(pwd)"
    echo "Building Lean oracle verifier..."
    cd verification/lean && lake build aura_verifier
    BINARY="$PROJECT_ROOT/verification/lean/.lake/build/bin/aura_verifier"
    [ -f "$BINARY" ] && echo "✓ Built: $BINARY" || { echo "✗ Binary not found"; exit 1; }

# Run differential tests against Lean oracle
test-differential: lean-oracle-build
    cargo test -p aura-testkit --features lean --test lean_differential -- --ignored --nocapture

# Check Lean proofs for completeness
lean-check jobs="4": (lean-build jobs)
    #!/usr/bin/env bash
    set -uo pipefail
    GREEN='\033[0;32m' YELLOW='\033[1;33m' NC='\033[0m'
    echo "Checking Lean proof status..."
    if grep -r "sorry" verification/lean/Aura --include="*.lean" > /tmp/sorry-check.txt 2>/dev/null; then
        count=$(wc -l < /tmp/sorry-check.txt | tr -d ' ')
        echo -e "${YELLOW}⚠ Found $count incomplete proofs (sorry)${NC}"
        head -10 /tmp/sorry-check.txt | sed 's/^/  /'
    else
        echo -e "${GREEN}✓ All proofs complete${NC}"
    fi

# Clean Lean build artifacts
lean-clean:
    cd verification/lean && lake clean

# Full Lean workflow (clean, build, verify)
lean-full: lean-clean lean-build lean-check
    @echo "Lean verification complete!"

# Show Lean proof status summary
lean-status:
    #!/usr/bin/env bash
    set -uo pipefail
    GREEN='\033[0;32m' YELLOW='\033[1;33m' NC='\033[0m'
    echo "Lean Proof Status"
    echo "================="
    find "verification/lean/Aura" -name "*.lean" -type f | sort | while read -r f; do
        name=$(basename "$f" .lean)
        sorries=$(grep -c "sorry" "$f" 2>/dev/null || echo 0)
        if [ "$sorries" -gt 0 ] 2>/dev/null; then
            echo -e "  ${YELLOW}○${NC} $name ($sorries incomplete)"
        else
            echo -e "  ${GREEN}●${NC} $name"
        fi
    done

# Translate Rust to Lean using Charon + Aeneas
lean-translate jobs="1" crate="all":
    #!/usr/bin/env bash
    set -uo pipefail
    GREEN='\033[0;32m' RED='\033[0;31m' NC='\033[0m'
    JOBS="{{jobs}}"; TARGET="{{crate}}"
    export CARGO_BUILD_JOBS="$JOBS" RUSTFLAGS="${RUSTFLAGS:-} -C codegen-units=1"

    echo "Translating Rust to Lean (jobs=$JOBS)"
    echo "======================================"
    mkdir -p verification/lean/Generated target/llbc

    command -v charon &>/dev/null || { echo -e "${RED}✗ Charon not found${NC}"; exit 1; }
    command -v aeneas &>/dev/null || { echo -e "${RED}✗ Aeneas not found${NC}"; exit 1; }

    CRATES=("aura-core" "aura-journal")
    [ "$TARGET" != "all" ] && CRATES=("$TARGET")

    for crate in "${CRATES[@]}"; do
        echo "Translating $crate..."
        llbc="target/llbc/${crate//-/_}.llbc"
        out="verification/lean/Generated/${crate//-/_}"
        mkdir -p "$out"
        nice -n 19 charon cargo --dest target/llbc -- -p "$crate" -j "$JOBS" 2>/dev/null && \
        nice -n 19 aeneas -backend lean "$llbc" -dest "$out" 2>/dev/null && \
            echo -e "  ${GREEN}✓${NC}" || echo -e "  ${RED}✗${NC}"
    done

# ═══════════════════════════════════════════════════════════════════════════════
# Kani Bounded Model Checking
# ═══════════════════════════════════════════════════════════════════════════════

# Run Kani verification on a package
kani package="aura-protocol" unwind="10":
    nix develop .#nightly --command cargo kani --package {{package}} --default-unwind {{unwind}}

# Run a specific Kani harness
kani-harness harness package="aura-protocol" unwind="10":
    nix develop .#nightly --command cargo kani --package {{package}} --harness {{harness}} --default-unwind {{unwind}}

# Setup Kani (first time only)
kani-setup:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Setting up Kani verifier..."
    nix develop .#nightly --command cargo install --locked kani-verifier
    nix develop .#nightly --command cargo kani setup
    echo "✓ Kani setup complete! Run 'just kani' to verify."

# Run full Kani verification suite
kani-suite:
    @./scripts/verify.sh kani

# ═══════════════════════════════════════════════════════════════════════════════
# Combined Verification
# ═══════════════════════════════════════════════════════════════════════════════

# Lean formal verification (matches CI lean-proofs job)
verify-lean:
    #!/usr/bin/env bash
    set -euo pipefail
    GREEN='\033[0;32m' RED='\033[0;31m' NC='\033[0m'
    echo "Lean Formal Verification"
    echo "========================"
    cd verification/lean
    lake build && echo -e "${GREEN}[OK]${NC} Lean proofs build" || { echo -e "${RED}[FAIL]${NC}"; exit 1; }
    cd ../..
    if grep -r "sorry" verification/lean/Aura --include="*.lean" 2>/dev/null; then
        echo "[WARNING] Found incomplete proofs (sorry)"
    else
        echo -e "${GREEN}[OK]${NC} All proofs complete"
    fi

# Consensus conformance tests (matches CI)
verify-conformance:
    #!/usr/bin/env bash
    set -euo pipefail
    GREEN='\033[0;32m' RED='\033[0;31m' NC='\033[0m'
    echo "Consensus Conformance Tests"
    echo "==========================="

    echo "[1/3] Generating ITF traces..."
    mkdir -p traces
    quint run --out-itf=traces/consensus.itf.json verification/quint/consensus/core.qnt \
        --max-steps=30 --max-samples=5 || { echo -e "${RED}[FAIL]${NC}"; exit 1; }
    echo -e "${GREEN}[OK]${NC} Generated traces"

    echo "[2/3] Running ITF conformance tests..."
    cargo test -p aura-protocol --test consensus_itf_conformance -- --nocapture || \
        { echo -e "${RED}[FAIL]${NC}"; exit 1; }
    echo -e "${GREEN}[OK]${NC} Conformance passed"

    echo "[3/3] Running differential tests..."
    cargo test -p aura-protocol --test consensus_differential -- --nocapture || \
        { echo -e "${RED}[FAIL]${NC}"; exit 1; }
    echo -e "${GREEN}[OK]${NC} Differential passed"

# Run all verification (Lean + Quint + Conformance)
verify-all: verify-lean quint-verify-models verify-conformance
    @echo ""; echo "ALL VERIFICATION COMPLETE"

# ═══════════════════════════════════════════════════════════════════════════════
# Scenarios
# ═══════════════════════════════════════════════════════════════════════════════

# Run scenarios with default settings
run-scenarios:
    cargo run --bin aura -- scenarios run --directory scenarios

# Run scenarios with specific pattern
run-scenarios-pattern pattern:
    cargo run --bin aura -- scenarios run --pattern {{pattern}} --directory scenarios

# Run scenarios in parallel
run-scenarios-parallel:
    cargo run --bin aura -- scenarios run --parallel --max-parallel 4 --directory scenarios

# List all available scenarios
list-scenarios:
    cargo run --bin aura -- scenarios list --directory scenarios --detailed

# Validate all scenarios
validate-scenarios:
    cargo run --bin aura -- scenarios validate --directory scenarios --strictness standard

# Discover scenarios in directory tree
discover-scenarios:
    cargo run --bin aura -- scenarios discover --root . --validate

# Generate HTML report from scenario execution results
generate-report input output:
    cargo run --bin aura -- scenarios report --input {{input}} --output {{output}} --format html --detailed

# Run full scenario test suite
test-scenarios: discover-scenarios validate-scenarios
    #!/usr/bin/env bash
    set -euo pipefail
    cargo run --bin aura -- scenarios run --directory scenarios --output-file outcomes/scenario_results.json --detailed-report
    just generate-report outcomes/scenario_results.json outcomes/scenario_report.html
    echo "Report: outcomes/scenario_report.html"

# ═══════════════════════════════════════════════════════════════════════════════
# Nix / Hermetic Builds
# ═══════════════════════════════════════════════════════════════════════════════

# Generate Cargo.nix using crate2nix
generate-cargo-nix:
    nix develop --command crate2nix generate
    @echo "Run 'nix build .#aura-terminal' to test hermetic build"

# Build using hermetic Nix build
build-nix:
    nix build .#aura-terminal

# Build specific package with hermetic Nix
build-nix-package package:
    nix build .#{{package}}

# Run hermetic Nix checks
check-nix:
    nix flake check

# Test hermetic build of all available packages
test-nix-all:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Testing all hermetic Nix builds..."
    for pkg in aura-terminal aura-agent aura-simulator; do
        echo "Building $pkg..."
        nix build .#$pkg && echo "[OK] $pkg"
    done
    echo "All hermetic builds completed!"

# ═══════════════════════════════════════════════════════════════════════════════
# WASM / Console
# ═══════════════════════════════════════════════════════════════════════════════

# Watch and serve the console frontend with hot reload
serve-console:
    cd crates/console && trunk serve --open

# Stop any running trunk servers
stop-console:
    pgrep -x trunk > /dev/null && pkill trunk && echo "Stopped trunk server" || echo "No trunk server running"

# Build WASM module for db-test
build-wasm-db-test:
    cd crates/db-test && wasm-pack build --target web --out-dir web/pkg
    @echo "Output: crates/db-test/web/pkg/"

# Serve the WASM test application
serve-wasm-db-test:
    #!/usr/bin/env bash
    set -euo pipefail
    [ -d "crates/db-test/web/pkg" ] || { echo "Run: just build-wasm-db-test"; exit 1; }
    echo "Server: http://localhost:8000"
    cd crates/db-test/web && python3 -m http.server 8000

# Build and serve WASM test
test-wasm-db: build-wasm-db-test serve-wasm-db-test

# ═══════════════════════════════════════════════════════════════════════════════
# Utilities
# ═══════════════════════════════════════════════════════════════════════════════

# Execute any aura CLI command
aura *ARGS='--help':
    @AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command cargo run --bin aura -- {{ARGS}}

# Show project statistics
stats:
    @echo "Project Statistics"
    @echo "=================="
    @echo "Lines of Rust code:"
    @find crates -name "*.rs" -type f -exec cat {} + | wc -l
    @echo "Number of crates:"
    @ls -1 crates | wc -l

# Install git hooks
install-hooks:
    @echo "#!/usr/bin/env bash" > .git/hooks/pre-commit
    @echo "just fmt-check && just clippy-strict" >> .git/hooks/pre-commit
    @chmod +x .git/hooks/pre-commit
    @echo "Git hooks installed"
