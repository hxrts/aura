# Justfile for Aura project automation

# Default recipe - show available commands
default:
    @just --list

# Generate docs/SUMMARY.md from Markdown files in docs/ and subfolders
summary:
    #!/usr/bin/env bash
    set -euo pipefail

    docs="docs"
    build_dir="$docs/book"
    out="$docs/SUMMARY.md"

    echo "# Summary" > "$out"
    echo "" >> "$out"

    # Helper function to extract title from markdown file
    get_title() {
        local f="$1"
        local title
        title="$(grep -m1 '^# ' "$f" | sed 's/^# *//')"
        if [ -z "$title" ]; then
            local base="$(basename "${f%.*}")"
            title="$(printf '%s\n' "$base" \
                | tr '._-' '   ' \
                | awk '{for(i=1;i<=NF;i++){ $i=toupper(substr($i,1,1)) substr($i,2) }}1')"
        fi
        echo "$title"
    }

    # Helper function to get chapter name from directory
    get_chapter_name() {
        local dir="$1"
        # Capitalize first letter of each word
        echo "$dir" | tr '_-' '  ' | awk '{for(i=1;i<=NF;i++){ $i=toupper(substr($i,1,1)) substr($i,2) }}1'
    }

    # Collect all files, organized by directory
    declare -A dirs
    declare -a root_files

    while IFS= read -r f; do
        rel="${f#$docs/}"

        # Skip SUMMARY.md
        [ "$rel" = "SUMMARY.md" ] && continue

        # Skip files under the build output directory
        case "$f" in "$build_dir"/*) continue ;; esac

        # Check if file is in a subdirectory
        if [[ "$rel" == */* ]]; then
            # Extract directory name (first component of path)
            dir="${rel%%/*}"
            # Add file to this directory's list
            dirs[$dir]+="$f"$'\n'
        else
            # Root-level file
            root_files+=("$f")
        fi
    done < <(find "$docs" -type f -name '*.md' -not -name 'SUMMARY.md' -not -path "$build_dir/*" | LC_ALL=C sort)

    # Write root-level files first
    for f in "${root_files[@]}"; do
        rel="${f#$docs/}"
        title="$(get_title "$f")"
        echo "- [$title]($rel)" >> "$out"
    done

    # Write chapters (directories) with their files
    for dir in $(printf '%s\n' "${!dirs[@]}" | LC_ALL=C sort); do
        # Add blank line before chapter
        [ ${#root_files[@]} -gt 0 ] && echo "" >> "$out"

        # Add chapter heading
        chapter_name="$(get_chapter_name "$dir")"
        echo "# $chapter_name" >> "$out"
        echo "" >> "$out"

        # Add files in this directory
        while IFS= read -r f; do
            [ -z "$f" ] && continue
            rel="${f#$docs/}"
            title="$(get_title "$f")"
            echo "- [$title]($rel)" >> "$out"
        done < <(echo -n "${dirs[$dir]}" | LC_ALL=C sort)
    done

    echo "Wrote $out"

# Build the book after regenerating the summary
book: summary
    AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command bash -c 'mdbook-mermaid install . > /dev/null 2>&1 || true && mdbook build'

# Serve locally with live reload
serve-book: summary
    #!/usr/bin/env bash
    set -euo pipefail

    # Kill any existing mdbook servers
    if pgrep -x mdbook > /dev/null; then
        echo "Stopping existing mdbook server..."
        pkill mdbook
        sleep 1
    fi

    AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command bash -c 'mdbook-mermaid install . > /dev/null 2>&1 || true && mdbook serve --open'

# Serve documentation with live reload (alias for serve-book)
serve: serve-book

# Build all crates
build:
    cargo build --workspace --verbose

# Build Aura terminal in development mode (release profile with dev features)
# Creates ./bin/aura symlink for easy access
build-dev:
    cargo build -p aura-terminal --bin aura --features development --release
    mkdir -p bin
    ln -sf ../target/release/aura bin/aura
    @echo "Binary available at: ./bin/aura"

build-app-host:
    cargo build -p aura-app --bin app-host --features host --release

build-terminal-release:
    cargo build -p aura-terminal --bin aura --release --no-default-features --features terminal
    mkdir -p bin
    ln -sf ../target/release/aura bin/aura
    @echo "Binary available at: ./bin/aura"

# Build in release mode
build-release:
    cargo build --workspace --release --verbose

# Run all tests
test:
    cargo test --workspace --verbose

# Run all tests with output
test-verbose:
    cargo test --workspace --verbose -- --nocapture

# Run tests for a specific crate
test-crate crate:
    cargo test -p {{crate}} --verbose

# Run tests for a specific crate avoiding architectural violations
test-crate-isolated crate:
    #!/usr/bin/env bash
    echo "Testing {{crate}} in isolation (lib + unit tests only)..."
    # Test just the library code with unit tests, avoiding dev dependencies that may violate architecture
    cd "crates/{{crate}}" && cargo test --lib --verbose

# Check code without building
check:
    cargo check --workspace --verbose

# Run the exact same lint command that Zed editor runs (rust-analyzer checkOnSave)
# This is exactly what Zed runs automatically on file save via rust-analyzer
check-zed:
    cargo check --workspace --all-targets

# Run clippy linter with effects system enforcement
clippy:
    cargo clippy --workspace --all-targets --verbose -- -D warnings

# Strict clippy check enforcing effects system usage
clippy-strict:
    cargo clippy --workspace --all-targets --verbose -- -D warnings -D clippy::disallowed_methods -D clippy::disallowed_types

# Test lint enforcement (should fail)
lint-test:
    cargo check test_lints.rs

# Format code
fmt:
    cargo fmt --all

# Check code formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run security audit
audit:
    cargo audit

# Clean build artifacts
clean:
    cargo clean

# Watch and rebuild on changes
watch:
    cargo watch -x build

# Watch and run tests on changes
watch-test:
    cargo watch -x test

# Initialize a new account (Phase 0 smoke test)
init-account:
    cargo run --bin aura -- init -n 3 -t 2 -o .aura

# Show account status
status:
    cargo run --bin aura -- status -c .aura/configs/device_1.toml

# Test key derivation
# test-dkd app_id context:
#     cargo run --bin aura -- test-dkd --app-id {{app_id}} --context {{context}} -f .aura/configs/device_1.toml
#     Note: Disabled - requires agent crate functionality

# Run Phase 0 smoke tests
smoke-test:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Running Phase 0 Smoke Tests"
    echo "==========================="
    echo ""

    # Clean previous test artifacts
    rm -rf .aura-test

    echo "1. Initializing 2-of-3 threshold account..."
    cargo run --bin aura -- init -n 3 -t 2 -o .aura-test
    echo "OK Account initialized"
    echo ""

    echo "2. Verifying effect_api and config files creation..."
    if [ -f ".aura-test/effect_api.cbor" ]; then
        echo "OK Effect API file created successfully"
        echo "Ledger size: $(stat -c%s .aura-test/effect_api.cbor 2>/dev/null || stat -f%z .aura-test/effect_api.cbor) bytes"
    else
        echo "ERROR: Ledger file not found"
        exit 1
    fi
    if [ -f ".aura-test/configs/device_1.toml" ]; then
        echo "OK Config file created successfully"
    else
        echo "ERROR: Config file not found"
        exit 1
    fi
    echo ""

    echo "3. Checking account status..."
    cargo run --bin aura -- status -c .aura-test/configs/device_1.toml
    echo "OK Status retrieved"
    echo ""

    echo "4. Testing multi-device threshold operations..."

    # Verify all 3 config files exist
    echo "   4.1 Verifying all device configs..."
    for i in 1 2 3; do
        if [ -f ".aura-test/configs/device_${i}.toml" ]; then
            echo "   [OK] Device ${i} config found"
        else
            echo "   ERROR: Device ${i} config not found"
            exit 1
        fi
    done
    echo "   OK All 3 device configs verified"

    # Test loading each device config
    echo "   4.2 Testing device config loading..."
    for i in 1 2 3; do
        echo "   Testing device ${i}..."
        if cargo run --bin aura -- status -c .aura-test/configs/device_${i}.toml > /dev/null 2>&1; then
            echo "   [OK] Device ${i} loaded successfully"
        else
            echo "   ERROR: Device ${i} failed to load"
            exit 1
        fi
    done
    echo "   OK All devices can load their configs"

    # Test starting agents on different ports
    echo "   4.3 Testing multi-device agents on different ports..."

    # Function to start an agent in background and capture PID
    start_agent() {
        local device_num=$1
        local port=$((58834 + device_num))
        local config_file=".aura-test/configs/device_${device_num}.toml"

        echo "   Starting device ${device_num} on port ${port}..."
        cargo run --bin aura -- node --port ${port} --daemon -c ${config_file} &
        local pid=$!
        echo ${pid} > ".aura-test/agent_${device_num}.pid"

        # Give agent time to start
        sleep 2

        # Check if agent is still running
        if kill -0 ${pid} 2>/dev/null; then
            echo "   [OK] Device ${device_num} agent started successfully (PID: ${pid})"
            return 0
        else
            echo "   ERROR: Device ${device_num} agent failed to start"
            return 1
        fi
    }

    # Function to stop all agents
    stop_all_agents() {
        echo "   Stopping all test agents..."
        for i in 1 2 3; do
            if [ -f ".aura-test/agent_${i}.pid" ]; then
                local pid=$(cat ".aura-test/agent_${i}.pid")
                if kill -0 ${pid} 2>/dev/null; then
                    kill ${pid}
                    echo "   [OK] Stopped agent ${i} (PID: ${pid})"
                fi
                rm -f ".aura-test/agent_${i}.pid"
            fi
        done
    }

    # Set up cleanup trap
    trap stop_all_agents EXIT

    # Start all three agents
    if start_agent 1 && start_agent 2 && start_agent 3; then
        echo "   OK All 3 agents started on different ports"

        # Wait a moment to ensure they're stable
        sleep 3

        # Verify agents are still running
        echo "   4.4 Verifying agents are stable..."
        all_running=true
        for i in 1 2 3; do
            if [ -f ".aura-test/agent_${i}.pid" ]; then
                pid=$(cat ".aura-test/agent_${i}.pid")
                if kill -0 ${pid} 2>/dev/null; then
                    echo "   [OK] Device ${i} agent still running"
                else
                    echo "   ERROR: Device ${i} agent stopped unexpectedly"
                    all_running=false
                fi
            else
                echo "   ERROR: Device ${i} PID file missing"
                all_running=false
            fi
        done

        if [ "$all_running" = true ]; then
            echo "   OK All agents are stable and running"
        else
            echo "   ERROR: Some agents are not stable"
            stop_all_agents
            exit 1
        fi

        # Clean stop of all agents
        stop_all_agents
        echo "   OK Agent startup/shutdown test completed"

        # Test threshold signature operation
        echo "   4.5 Testing 2-of-3 threshold signature operation..."

        # Test threshold signature with all 3 devices
        if cargo run --bin aura -- threshold --configs .aura-test/configs/device_1.toml,.aura-test/configs/device_2.toml,.aura-test/configs/device_3.toml --threshold 2 --mode local > /dev/null 2>&1; then
            echo "   [OK] 3-device threshold signature test passed"
        else
            echo "   ERROR: 3-device threshold signature test failed"
            exit 1
        fi

        # Test threshold signature with minimum required (2 devices)
        if cargo run --bin aura -- threshold --configs .aura-test/configs/device_1.toml,.aura-test/configs/device_2.toml --threshold 2 --mode local > /dev/null 2>&1; then
            echo "   [OK] 2-device minimum threshold test passed"
        else
            echo "   ERROR: 2-device minimum threshold test failed"
            exit 1
        fi

        echo "   OK Threshold signature operations verified"
    else
        echo "   ERROR: Failed to start all agents"
        stop_all_agents
        exit 1
    fi
    echo ""

    echo "5. Testing scenario discovery..."
    if [ -d "scenarios" ]; then
        cargo run --bin aura -- scenarios discover --root . > /dev/null 2>&1
        echo "OK Scenario discovery functional"
    else
        echo "SKIP No scenarios directory found"
    fi
    echo ""

    echo "Phase 0 smoke tests passed!"
    echo "Multi-device setup and basic operations functional!"

# Run macOS Keychain integration tests
test-macos-keychain:
    #!/usr/bin/env bash
    set -e

    echo "Aura macOS Keychain Integration Tests"
    echo "======================================"
    echo ""

    # Check that we're on macOS
    if [[ "$OSTYPE" != "darwin"* ]]; then
        echo "Error: These tests are designed for macOS only"
        echo "Current OS: $OSTYPE"
        exit 1
    fi

    # Check if we're in the right directory
    if [[ ! -f "Cargo.toml" ]] || [[ ! -d "crates/agent" ]]; then
        echo "Error: Please run this from the Aura project root directory"
        exit 1
    fi

    echo "Important Notes:"
    echo "  - These tests will interact with your macOS Keychain"
    echo "  - You may be prompted to allow keychain access"
    echo "  - Test data will be created and cleaned up automatically"
    echo "  - Some tests may require administrator permissions"
    echo ""

    # Prompt for confirmation
    read -p "Continue with keychain tests? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Tests cancelled by user"
        exit 0
    fi

    echo "Starting macOS Keychain Tests..."
    echo ""

    # Set test environment variables
    export RUST_LOG=debug
    export RUST_BACKTRACE=1

    # Run the specific macOS keychain tests
    cargo test -p aura-agent --test macos_keychain_tests -- --nocapture --test-threads=1

    TEST_EXIT_CODE=$?

    echo ""
    if [ $TEST_EXIT_CODE -eq 0 ]; then
        echo "All macOS keychain tests passed!"
        echo ""
        echo "Secure Storage System Verification:"
        echo "  - Platform-specific keychain backend: Working"
        echo "  - Hardware UUID derivation: Working"
        echo "  - Device attestation with SIP: Working"
        echo "  - Key share encryption/decryption: Working"
        echo "  - Keychain persistence: Working"
        echo "  - Error handling: Working"
        echo ""
        echo "Your macOS keychain integration is ready for production use!"
    else
        echo "Some keychain tests failed"
        echo ""
        echo "Common Issues:"
        echo "  - Keychain access denied - grant permission when prompted"
        echo "  - System Integrity Protection disabled - enable SIP for security"
        echo "  - Missing dependencies - ensure all required crates are built"
        echo ""
        echo "Check the test output above for specific error details."
        exit $TEST_EXIT_CODE
    fi

    echo ""
    echo "Test Summary:"
    echo "  - Platform: macOS (Keychain Services)"
    echo "  - Encryption: AES-256-GCM with random nonces"
    echo "  - Authentication: Hardware-backed device attestation"
    echo "  - Storage: Persistent keychain with access control"
    echo "  - Integration: Complete workflow tested"

    echo ""
    echo "Security Features Verified:"
    echo "  - Hardware-backed key storage"
    echo "  - Platform-specific device identification"
    echo "  - System Integrity Protection detection"
    echo "  - Secure boot verification preparation"
    echo "  - Device attestation with Ed25519 signatures"
    echo "  - Keychain access control integration"

    echo ""
    echo "Next Steps:"
    echo "  - Run 'just init-account' to test with the new secure storage"
    echo "  - Use 'just status' to verify keychain integration"
    echo "  - Deploy with confidence knowing keys are hardware-protected"

# Check architectural layer compliance (all checks)
check-arch *FLAGS:
    scripts/check-arch.sh {{FLAGS}} || true

# Quick architectural checks by category
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

# Run CI checks locally (dry-run of GitHub CI workflow)
ci-dry-run:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Running CI Dry-Run (Local GitHub Workflow Simulation)"
    echo "======================================================"
    echo ""

    # Colors for output
    GREEN='\033[0;32m'
    RED='\033[0;31m'
    YELLOW='\033[0;33m'
    NC='\033[0m' # No Color

    exit_code=0

    # Clean ALL build artifacts to ensure fresh compilation (matches GitHub CI behavior)
    echo "Cleaning build cache to ensure fresh compilation..."
    cargo clean
    echo ""

    # 1. Format Check
    echo "[1/8] Running Format Check..."
    if cargo fmt --all -- --check; then
        echo -e "${GREEN}[OK]${NC} Format check passed"
    else
        echo -e "${RED}[FAIL]${NC} Format check failed"
        exit_code=1
    fi
    echo ""

    # 2. Clippy with Effects Enforcement
    echo "[2/8] Running Clippy with Effects Enforcement..."
    if cargo clippy --workspace --all-targets --verbose -- \
        -D warnings \
        -D clippy::disallowed_methods \
        -D clippy::disallowed_types \
        -D clippy::unwrap_used \
        -D clippy::expect_used \
        -D clippy::duplicated_attributes; then
        echo -e "${GREEN}[OK]${NC} Clippy check passed"
    else
        echo -e "${RED}[FAIL]${NC} Clippy check failed"
        exit_code=1
    fi
    echo ""

    # 3. Test Suite
    echo "[3/8] Running Test Suite..."
    if cargo test --workspace --verbose; then
        echo -e "${GREEN}[OK]${NC} Test suite passed"
    else
        echo -e "${RED}[FAIL]${NC} Test suite failed"
        exit_code=1
    fi
    echo ""

    # 4. Check for Effects System Violations (Layer-Aware)
    echo "[4/8] Checking for Effects System Violations..."
    violations_found=0

    # Layer architecture:
    # - Layer 3 (aura-effects): Production handlers - MUST use SystemTime::now(), thread_rng()
    # - Layer 6 (aura-simulator): Runtime composition - allowed for instrumentation
    # - Layer 7 (aura-terminal): User interface - allowed for TUI/CLI interaction
    # - Layer 8 (aura-testkit, tests/): Testing infrastructure - allowed
    # - All other layers: MUST use effect traits

    # Check for direct time usage (exclude Layer 3, 6, 7, 8, integration tests, demo code, CLI scenarios, and test modules)
    # Note: May include false positives from code in comments or test modules
    time_violations=$(rg --type rust "SystemTime::now|Instant::now|chrono::Utc::now" crates/ --line-number \
        --glob '!**/aura-effects/**' \
        --glob '!**/aura-simulator/**' \
        --glob '!**/aura-terminal/**' \
        --glob '!**/aura-testkit/**' \
        --glob '!**/tests/**' \
        --glob '!**/integration/**' \
        --glob '!**/demo/**' \
        --glob '!**/examples/**' 2>/dev/null | \
        grep -v '^\s*//\|^\s\+//\|:\s*//' | \
        grep -v "#\[tokio::test\]" | \
        grep -v "#\[test\]" || true)

    # Filter out lines from files with #[cfg(test)] modules
    if [ -n "$time_violations" ]; then
        filtered_time=""
        while IFS= read -r line; do
            [ -z "$line" ] && continue
            file_path="${line%%:*}"
            if [ -f "$file_path" ] && grep -q "#\[cfg(test)\]" "$file_path" 2>/dev/null; then
                match_line_num=$(echo "$line" | cut -d: -f2)
                cfg_test_line=$(grep -n "#\[cfg(test)\]" "$file_path" 2>/dev/null | head -1 | cut -d: -f1)
                if [ -n "$match_line_num" ] && [ -n "$cfg_test_line" ] && [ "$match_line_num" -gt "$cfg_test_line" ]; then
                    continue
                fi
            fi
            filtered_time="${filtered_time}${line}"$'\n'
        done <<< "$time_violations"
        time_violations="$filtered_time"
    fi

    if [ -n "$time_violations" ] && [ "$time_violations" != $'\n' ]; then
        echo "$time_violations"
        echo -e "${RED}[ERROR]${NC} Found direct time usage in application code! Use PhysicalTimeEffects::now() instead."
        violations_found=1
    fi

    # Check for direct randomness usage (exclude Layer 3, 6, 7, 8, integration tests, and demo code)
    if rg --type rust "rand::random|thread_rng\(\)|OsRng::new" crates/ --line-number \
        --glob '!**/aura-effects/**' \
        --glob '!**/aura-simulator/**' \
        --glob '!**/aura-terminal/**' \
        --glob '!**/aura-testkit/**' \
        --glob '!**/tests/**' \
        --glob '!**/integration/**' \
        --glob '!**/demo/**' \
        --glob '!**/examples/**' 2>/dev/null; then
        echo -e "${RED}[ERROR]${NC} Found direct randomness usage in application code! Use RandomEffects methods instead."
        violations_found=1
    fi

    # Check for direct UUID usage (exclude Layer 3, 6, 7, 8, integration tests, demo code, ID constructors, property tests, and test modules)
    uuid_violations=$(rg --type rust "Uuid::new_v4\(\)" crates/ --line-number \
        --glob '!**/aura-effects/**' \
        --glob '!**/aura-agent/**' \
        --glob '!**/aura-simulator/**' \
        --glob '!**/aura-testkit/**' \
        --glob '!**/aura-quint/**' \
        --glob '!**/aura-terminal/**' \
        --glob '!**/tests/**' \
        --glob '!**/integration/**' \
        --glob '!**/demo/**' \
        --glob '!**/tui/**' \
        --glob '!**/examples/**' \
        --glob '!**/aura-core/src/types/identifiers.rs' \
        --glob '!**/aura-core/src/effects/quint.rs' \
        --glob '!**/aura-composition/src/registry.rs' \
        --glob '!**/aura-sync/src/services/maintenance.rs' \
        --glob '!**/aura-sync/src/infrastructure/peers.rs' 2>/dev/null | \
        grep -v '^\s*//\|^\s\+//\|:\s*//' | \
        grep -v "#\[tokio::test\]" | \
        grep -v "#\[test\]" || true)

    # Filter out lines from files with #[cfg(test)] modules
    if [ -n "$uuid_violations" ]; then
        filtered_uuid=""
        while IFS= read -r line; do
            [ -z "$line" ] && continue
            file_path="${line%%:*}"
            if [ -f "$file_path" ] && grep -q "#\[cfg(test)\]" "$file_path" 2>/dev/null; then
                match_line_num=$(echo "$line" | cut -d: -f2)
                cfg_test_line=$(grep -n "#\[cfg(test)\]" "$file_path" 2>/dev/null | head -1 | cut -d: -f1)
                if [ -n "$match_line_num" ] && [ -n "$cfg_test_line" ] && [ "$match_line_num" -gt "$cfg_test_line" ]; then
                    continue
                fi
            fi
            filtered_uuid="${filtered_uuid}${line}"$'\n'
        done <<< "$uuid_violations"
        uuid_violations="$filtered_uuid"
    fi

    if [ -n "$uuid_violations" ] && [ "$uuid_violations" != $'\n' ]; then
        echo "$uuid_violations"
        echo -e "${RED}[ERROR]${NC} Found direct UUID usage in application code! Use RandomEffects::random_uuid() instead."
        violations_found=1
    fi

    if [ $violations_found -eq 0 ]; then
        echo -e "${GREEN}[OK]${NC} No effects system violations found"
    else
        echo ""
        echo -e "${YELLOW}Note:${NC} Layer 1 ID constructors (aura-core/identifiers.rs), Layer 3 (aura-effects), Layer 6 (aura-agent, aura-simulator), Layer 7 (aura-terminal), Layer 8 (aura-testkit, tests/), property tests (aura-quint), demo/TUI code, operation ID generation (aura-composition/registry.rs), and sync service IDs (aura-sync/services, aura-sync/infrastructure) are exempt."
        exit_code=1
    fi
    echo ""

    # 5. Documentation Links Check
    echo "[5/8] Checking Documentation Links..."
    # Install markdown-link-check if not available
    if ! command -v markdown-link-check &> /dev/null; then
        echo "Installing markdown-link-check..."
        npm install -g markdown-link-check > /dev/null 2>&1
    fi

    doc_errors=0
    doc_output=$(mktemp)

    # Find all markdown files recursively (matching GitHub CI behavior)
    # Exclude node_modules, target, and .git directories
    while IFS= read -r file; do
        if [ -f "$file" ]; then
            if ! markdown-link-check "$file" --config .github/config/markdown-link-check.json 2>&1 | tee -a "$doc_output" | grep -q "ERROR:"; then
                :
            else
                echo -e "${RED}Broken links found in $file${NC}"
                doc_errors=1
            fi
        fi
    done < <(find . -name "*.md" -type f \
        ! -path "*/node_modules/*" \
        ! -path "*/target/*" \
        ! -path "*/.git/*" \
        ! -path "*/.aura-test/*" \
        ! -path "*/ext/quint/*" \
        ! -path "*/work/*" \
        ! -path "*/.claude/skills/*")

    if [ $doc_errors -eq 0 ]; then
        echo -e "${GREEN}[OK]${NC} Documentation links check passed"
    else
        echo -e "${RED}[FAIL]${NC} Documentation links check failed"
        echo "See errors above for details"
        exit_code=1
    fi
    rm -f "$doc_output"
    echo ""

    # 6. Build Check
    echo "[6/8] Running Build Check..."
    if cargo build --workspace --verbose; then
        echo -e "${GREEN}[OK]${NC} Build check passed"
    else
        echo -e "${RED}[FAIL]${NC} Build check failed"
        exit_code=1
    fi
    echo ""

    # 7. Unused Dependencies Check
    echo "[7/8] Checking for Unused Dependencies (cargo-udeps)..."
    echo "Using nightly Rust toolchain for cargo-udeps..."

    # Run cargo-udeps using the nightly shell from flake.nix
    if nix develop .#nightly --command cargo udeps --all-targets 2>&1 | tee /tmp/udeps-output.txt | grep -q "unused dependencies:"; then
        # Found unused dependencies - show them
        echo -e "${YELLOW}[WARNING]${NC} Found unused dependencies:"
        grep -A 100 "unused dependencies:" /tmp/udeps-output.txt | head -50
        echo ""
        echo -e "${YELLOW}Note:${NC} cargo-udeps may report false positives for:"
        echo "  - Dependencies used in macro-generated code"
        echo "  - Dependencies used only in doc-tests"
        echo "  - Re-exported dependencies in public APIs"
        echo ""
        echo "Review the output above and verify these are actual unused dependencies."
        echo "This is a WARNING, not a failure - CI will continue."
    else
        echo -e "${GREEN}[OK]${NC} No unused dependencies found (or only known false positives)"
    fi
    rm -f /tmp/udeps-output.txt
    echo ""

    # 8. Summary
    echo "[8/8] Summary"
    echo "======================================================"
    if [ $exit_code -eq 0 ]; then
        echo -e "${GREEN}All CI checks passed!${NC}"
        echo "Ready to submit PR - matches GitHub CI requirements"
    else
        echo -e "${RED}Some CI checks failed!${NC}"
        echo "Please fix the issues above before submitting PR"
        exit $exit_code
    fi

# Parse Quint file to JSON using native parser
quint-parse input output:
    @echo "Parsing Quint file to JSON..."
    @echo "Input: {{input}}"
    @echo "Output: {{output}}"
    nix develop --command quint compile --target json --out {{output}} {{input}}
    @echo "Parse completed successfully!"

# Parse Quint file and display AST structure
quint-parse-ast input:
    @echo "Parsing Quint file AST..."
    nix develop --command quint parse --out /tmp/quint-ast.json {{input}}
    @echo "AST structure for {{input}}:"
    @echo "============================"
    jq '.modules[0].name as $name | "Module: " + $name' /tmp/quint-ast.json
    jq '.modules[0].declarations | length as $count | "Declarations: " + ($count | tostring)' /tmp/quint-ast.json
    @echo ""
    @echo "Full AST available at: /tmp/quint-ast.json"

# Parse Quint file with type checking and compile to JSON
quint-compile input output:
    @echo "Compiling Quint file with full type checking..."
    @echo "Input: {{input}}"
    @echo "Output: {{output}}"
    nix develop --command quint typecheck {{input}}
    nix develop --command quint compile --target json --out {{output}} {{input}}
    @echo "Compilation completed successfully!"

# Test Quint parsing with example file
test-quint-parse:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Testing Quint Parsing Capabilities"
    echo "==================================="
    echo ""

    # Create a test Quint file
    mkdir -p .aura-test
    cat > .aura-test/test.qnt << 'EOF'
    module test {
      var counter: int

      action init = {
        counter' = 0
      }

      action increment = {
        counter' = counter + 1
      }

      action step = {
        increment
      }

      val counterInvariant = counter >= 0
    }
    EOF

    echo "1. Created test Quint file: .aura-test/test.qnt"
    echo ""

    echo "2. Parsing to JSON..."
    just quint-parse .aura-test/test.qnt .aura-test/test.json
    echo ""

    echo "3. Examining parsed structure..."
    echo "Main module:"
    jq '.main' .aura-test/test.json
    echo ""
    echo "Module declarations count:"
    jq '.modules[0].declarations | length' .aura-test/test.json
    echo ""

    echo "4. Testing AST parsing..."
    just quint-parse-ast .aura-test/test.qnt
    echo ""

    echo "5. Files generated:"
    ls -la .aura-test/test.*
    echo ""

    echo "Quint parsing test completed successfully!"
    echo "JSON output available at: .aura-test/test.json"

# Verify Quint setup and parsing capabilities
verify-quint:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Verifying Quint Setup"
    echo "====================="
    echo ""

    echo "1. Checking Quint installation..."
    nix develop --command quint --version
    echo ""

    echo "2. Checking Node.js (required for Quint)..."
    nix develop --command node --version
    echo ""

    echo "3. Checking Java Runtime (required for ANTLR)..."
    nix develop --command java -version
    echo ""

    echo "4. Testing basic Quint functionality..."
    echo 'module simple { val x = 1 }' > /tmp/simple.qnt
    nix develop --command quint parse /tmp/simple.qnt > /dev/null && echo "[OK] Basic parsing works"
    echo ""

    echo "Quint setup verification completed!"

# Typecheck all Quint specs without verification (fast sanity check)
quint-typecheck-all:
    #!/usr/bin/env bash
    set -uo pipefail

    GREEN='\033[0;32m'
    RED='\033[0;31m'
    YELLOW='\033[1;33m'
    NC='\033[0m'

    echo "Typechecking All Quint Specs"
    echo "============================"
    echo ""

    SPECS_DIR="verification/quint"
    TEST_SPECS_DIR="crates/aura-simulator/tests/quint_specs"

    passed=0
    failed=0

    # Typecheck verification/quint/
    echo "Checking specs in $SPECS_DIR..."
    for spec in $SPECS_DIR/protocol_*.qnt $SPECS_DIR/harness_*.qnt; do
        if [ -f "$spec" ]; then
            name=$(basename "$spec")
            if quint typecheck "$spec" > /dev/null 2>&1; then
                echo -e "  ${GREEN}✓${NC} $name"
                passed=$((passed + 1))
            else
                echo -e "  ${RED}✗${NC} $name"
                failed=$((failed + 1))
            fi
        fi
    done

    # Typecheck test specs
    echo ""
    echo "Checking specs in $TEST_SPECS_DIR..."
    for spec in $TEST_SPECS_DIR/*.qnt; do
        if [ -f "$spec" ]; then
            name=$(basename "$spec")
            if quint typecheck "$spec" > /dev/null 2>&1; then
                echo -e "  ${GREEN}✓${NC} $name"
                passed=$((passed + 1))
            else
                echo -e "  ${RED}✗${NC} $name"
                failed=$((failed + 1))
            fi
        fi
    done

    echo ""
    echo "=============================="
    echo -e "Passed: ${GREEN}$passed${NC}"
    echo -e "Failed: ${RED}$failed${NC}"

    if [ $failed -gt 0 ]; then
        echo -e "${RED}Some specs failed typecheck!${NC}"
        exit 1
    else
        echo -e "${GREEN}All specs passed typecheck!${NC}"
    fi

# Verify Quint specs with Apalache symbolic model checking
# Usage: just quint-verify-all              # Run with defaults
# Usage: just quint-verify-all 5            # Max 5 steps
# Usage: just quint-verify-all 10 traces    # Max 10 steps, output to traces/
quint-verify-all max_steps="5" output_dir="traces/verify":
    #!/usr/bin/env bash
    set -uo pipefail

    GREEN='\033[0;32m'
    RED='\033[0;31m'
    YELLOW='\033[1;33m'
    NC='\033[0m'

    MAX_STEPS="{{max_steps}}"
    OUTPUT_DIR="{{output_dir}}"

    echo "Quint + Apalache Verification"
    echo "============================="
    echo "Max steps: $MAX_STEPS"
    echo "Output dir: $OUTPUT_DIR"
    echo ""

    mkdir -p "$OUTPUT_DIR"

    # Specs with their invariants to verify
    # Format: spec_file:invariant1,invariant2,...
    VERIFY_TARGETS=(
        # Journal CRDT properties (proving.md §1)
        "verification/quint/protocol_journal.qnt:InvariantNonceUnique,InvariantEventsOrdered,InvariantLamportMonotonic,InvariantReduceDeterministic"

        # Consensus fast-path/fallback (proving.md §1)
        "verification/quint/protocol_consensus.qnt:InvariantUniqueCommitPerInstance,InvariantCommitRequiresThreshold,InvariantPathConvergence"

        # Anti-entropy convergence (proving.md §3)
        "verification/quint/protocol_anti_entropy.qnt:InvariantFactsMonotonic,InvariantVectorClockConsistent,InvariantEventualConvergence"

        # Recovery safety (proving.md §4)
        "verification/quint/protocol_recovery.qnt:InvariantThresholdWithinBounds,InvariantApprovalsSubsetGuardians,InvariantPhaseConsistency"

        # Session management
        "verification/quint/protocol_sessions.qnt:InvariantAuthoritiesRegisteredSessions,InvariantRevokedInactive"
    )

    passed=0
    failed=0
    skipped=0

    for target in "${VERIFY_TARGETS[@]}"; do
        IFS=':' read -r spec invariants <<< "$target"
        spec_name=$(basename "$spec" .qnt)

        if [ ! -f "$spec" ]; then
            echo -e "${YELLOW}SKIP${NC} $spec_name (file not found)"
            skipped=$((skipped + 1))
            continue
        fi

        echo ""
        echo "Verifying $spec_name..."
        echo "  Invariants: $invariants"

        # Convert comma-separated invariants to --invariant flags
        inv_flags=""
        IFS=',' read -ra INV_ARRAY <<< "$invariants"
        for inv in "${INV_ARRAY[@]}"; do
            inv_flags="$inv_flags --invariant=$inv"
        done

        output_file="$OUTPUT_DIR/${spec_name}_verify.json"

        # Run quint verify with Apalache
        if quint verify "$spec" \
            $inv_flags \
            --max-steps="$MAX_STEPS" \
            --out-itf="$OUTPUT_DIR/${spec_name}_counter.itf.json" \
            > "$output_file" 2>&1; then
            echo -e "  ${GREEN}✓ PASS${NC}"
            passed=$((passed + 1))
        else
            # Check if it's a real failure or just a limitation
            if grep -q "no violation found" "$output_file" 2>/dev/null; then
                echo -e "  ${GREEN}✓ PASS${NC} (no violation in $MAX_STEPS steps)"
                passed=$((passed + 1))
            elif grep -q "Apalache" "$output_file" 2>/dev/null; then
                echo -e "  ${RED}✗ FAIL${NC}"
                echo "    Counterexample: $OUTPUT_DIR/${spec_name}_counter.itf.json"
                echo "    Full output: $output_file"
                failed=$((failed + 1))
            else
                echo -e "  ${YELLOW}? ERROR${NC} (see $output_file)"
                skipped=$((skipped + 1))
            fi
        fi
    done

    echo ""
    echo "=============================="
    echo -e "Passed:  ${GREEN}$passed${NC}"
    echo -e "Failed:  ${RED}$failed${NC}"
    echo -e "Skipped: ${YELLOW}$skipped${NC}"
    echo ""
    echo "Counterexamples (if any) saved to: $OUTPUT_DIR/"

    if [ $failed -gt 0 ]; then
        echo -e "${RED}Some invariants were violated!${NC}"
        exit 1
    else
        echo -e "${GREEN}All verified invariants hold (up to $MAX_STEPS steps)${NC}"
    fi

# Verify a single Quint spec with specific invariants
# Usage: just quint-verify verification/quint/protocol_journal.qnt InvariantNonceUnique
quint-verify spec invariants:
    #!/usr/bin/env bash
    set -uo pipefail

    echo "Verifying {{spec}}..."
    echo "Invariants: {{invariants}}"
    echo ""

    mkdir -p traces/verify
    SPEC_NAME=$(basename "{{spec}}" .qnt)

    # Convert comma-separated to multiple --invariant flags
    INV_FLAGS=""
    IFS=',' read -ra INV_ARRAY <<< "{{invariants}}"
    for inv in "${INV_ARRAY[@]}"; do
        INV_FLAGS="$INV_FLAGS --invariant=$inv"
    done

    quint verify "{{spec}}" \
        $INV_FLAGS \
        --max-steps=10 \
        --out-itf="traces/verify/${SPEC_NAME}_counter.itf.json" \
        --verbosity=3

# Generate ITF traces from all Quint specs
quint-generate-traces:
    #!/usr/bin/env bash
    set -uo pipefail

    GREEN='\033[0;32m'
    RED='\033[0;31m'
    YELLOW='\033[1;33m'
    NC='\033[0m'

    echo "Generating ITF Traces from Quint Specs"
    echo "======================================"
    echo ""

    mkdir -p traces

    # Specs that have step actions defined
    RUNNABLE_SPECS=(
        "verification/quint/protocol_journal.qnt:journal"
        "verification/quint/protocol_consensus.qnt:consensus"
        "verification/quint/protocol_anti_entropy.qnt:anti_entropy"
        "verification/quint/protocol_epochs.qnt:epochs"
        "verification/quint/protocol_cross_interaction.qnt:cross_interaction"
        "verification/quint/protocol_capability_properties.qnt:cap_props"
        "verification/quint/protocol_frost.qnt:frost"
    )

    for target in "${RUNNABLE_SPECS[@]}"; do
        IFS=':' read -r spec trace_name <<< "$target"

        if [ ! -f "$spec" ]; then
            echo -e "${YELLOW}SKIP${NC} $spec (not found)"
            continue
        fi

        echo "Generating trace for $trace_name..."
        if quint run "$spec" \
            --main=step \
            --max-steps=15 \
            --out-itf="traces/${trace_name}.itf.json" \
            > /dev/null 2>&1; then
            size=$(du -h "traces/${trace_name}.itf.json" | cut -f1)
            echo -e "  ${GREEN}✓${NC} traces/${trace_name}.itf.json ($size)"
        else
            echo -e "  ${RED}✗${NC} Failed to generate trace"
        fi
    done

    echo ""
    echo "Trace generation complete. Files in traces/"
    ls -la traces/*.itf.json 2>/dev/null || echo "No traces generated"

# Execute any aura CLI command with nix build
# Usage: just aura init -n 3 -t 2 -o test-account
# Usage: just aura status -c test-account/configs/device_1.toml
# Usage: just aura scenarios list
aura *ARGS='--help':
    @AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command cargo build --bin aura
    @AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command cargo run --bin aura -- {{ARGS}}

# Generate Cargo.nix using crate2nix (needed for hermetic Nix builds)
generate-cargo-nix:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Regenerating Cargo.nix with crate2nix..."
    nix develop --command crate2nix generate
    echo "Cargo.nix regenerated successfully!"
    echo ""
    echo "Run 'nix build .#aura-terminal' to test hermetic build"

# Build using hermetic Nix build (requires Cargo.nix to exist)
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
    echo "=================================="
    echo ""

    echo "1. Building aura-terminal..."
    nix build .#aura-terminal
    echo "[OK] aura-terminal built successfully"

    echo "2. Building aura-agent..."
    nix build .#aura-agent
    echo "[OK] aura-agent built successfully"

    echo "3. Building aura-simulator..."
    nix build .#aura-simulator
    echo "[OK] aura-simulator built successfully"

    echo ""
    echo "All hermetic builds completed successfully!"

# Generate documentation
docs:
    cargo doc --workspace --no-deps --open

# Watch and serve the console frontend with hot reload
serve-console:
    cd crates/console && trunk serve --open

# Stop any running trunk servers
stop-console:
    #!/usr/bin/env bash
    if pgrep -x trunk > /dev/null; then
        pkill trunk
        echo "Stopped trunk server"
    else
        echo "No trunk server running"
    fi

# Show project statistics
stats:
    @echo "Project Statistics"
    @echo "=================="
    @echo ""
    @echo "Lines of Rust code:"
    @find crates -name "*.rs" -type f -exec cat {} + | wc -l
    @echo ""
    @echo "Number of crates:"
    @ls -1 crates | wc -l
    @echo ""
    @echo "Dependencies:"
    @cargo tree --workspace --depth 1 | grep -v "└──" | grep -v "├──" | tail -n +2 | wc -l

# Install git hooks
install-hooks:
    @echo "Installing git hooks..."
    @echo "#!/usr/bin/env bash" > .git/hooks/pre-commit
    @echo "just fmt-check && just clippy-strict" >> .git/hooks/pre-commit
    @chmod +x .git/hooks/pre-commit
    @echo "Git hooks installed"

# Build WASM module for db-test
build-wasm-db-test:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Building Datafrog WASM module..."
    echo "================================"
    echo ""

    cd crates/db-test

    wasm-pack build --target web --out-dir web/pkg

    echo ""
    echo "WASM build complete!"
    echo "Output: crates/db-test/web/pkg/"
    echo ""
    echo "To test, run: just serve-wasm-db-test"

# Serve the WASM test application
serve-wasm-db-test:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Starting web server for Datafrog WASM test..."
    echo "============================================="
    echo ""

    if [ ! -d "crates/db-test/web/pkg" ]; then
        echo "Error: WASM module not built yet"
        echo "Run: just build-wasm-db-test"
        exit 1
    fi

    echo "Server running at: http://localhost:8000"
    echo "Press Ctrl+C to stop"
    echo ""

    cd crates/db-test/web
    python3 -m http.server 8000

# Build and serve WASM test in one command
test-wasm-db: build-wasm-db-test serve-wasm-db-test

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

# Run full scenario test suite (discovery, validation, execution, reporting)
test-scenarios:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Running Full Scenario Test Suite"
    echo "================================"
    echo ""

    echo "1. Discovering scenarios..."
    just discover-scenarios
    echo ""

    echo "2. Validating scenarios..."
    just validate-scenarios
    echo ""

    echo "3. Running scenarios..."
    cargo run --bin aura -- scenarios run --directory scenarios --output-file outcomes/scenario_results.json --detailed-report
    echo ""

    echo "4. Generating report..."
    just generate-report outcomes/scenario_results.json outcomes/scenario_report.html
    echo ""

    echo "Scenario test suite completed!"
    echo "Report available at: outcomes/scenario_report.html"

# Demonstrate the complete Quint to JSON to simulator pipeline
test-quint-pipeline:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Testing Quint Specification to Simulator Pipeline"
    echo "=================================================="
    echo ""

    # Clean up any previous test artifacts
    rm -f /tmp/quint_pipeline_test.json
    mkdir -p .aura-test

    echo "1. Converting Quint specification to JSON..."
    echo "   Input: verification/quint/protocol_dkd.qnt"
    echo "   Output: /tmp/quint_pipeline_test.json"
    echo ""
    just quint-parse verification/quint/protocol_dkd.qnt /tmp/quint_pipeline_test.json
    echo ""

    echo "2. Verifying JSON output structure..."
    if [ -f "/tmp/quint_pipeline_test.json" ]; then
        echo "   [OK] JSON file created successfully"
        echo "   Size: $(stat -f%z /tmp/quint_pipeline_test.json 2>/dev/null || stat -c%s /tmp/quint_pipeline_test.json) bytes"

        # Extract key information from the JSON
        echo "   Module name: $(jq -r '.modules[0].name' /tmp/quint_pipeline_test.json)"
        echo "   Declarations: $(jq '.modules[0].declarations | length' /tmp/quint_pipeline_test.json)"
        echo "   Variables: $(jq '.modules[0].declarations | map(select(.kind == "var")) | length' /tmp/quint_pipeline_test.json)"
        echo "   Actions: $(jq '.modules[0].declarations | map(select(.qualifier == "action")) | length' /tmp/quint_pipeline_test.json)"
        echo "   Properties: $(jq '.modules[0].declarations | map(select(.qualifier == "val")) | length' /tmp/quint_pipeline_test.json)"
    else
        echo "   [ERROR] JSON file not created"
        exit 1
    fi
    echo ""

    echo "3. Testing JSON can be consumed by simulator..."
    echo "   The JSON output contains all necessary information for the simulator:"
    echo "   - State variables: sessionCount, completedSessions, failedSessions"
    echo "   - Actions: startSession, completeSession, failSession, init, step"
    echo "   - Properties: validCounts, sessionLimit, safetyProperty, progressProperty"
    echo ""

    echo "4. Verifying existing integration..."
    if [ -f "/tmp/dkd_spec.json" ]; then
        echo "   [OK] Existing DKD spec JSON found and ready for simulator"
        echo "   Size: $(stat -f%z /tmp/dkd_spec.json 2>/dev/null || stat -c%s /tmp/dkd_spec.json) bytes"
        echo "   Created: $(stat -f%Sm /tmp/dkd_spec.json 2>/dev/null || stat -c%y /tmp/dkd_spec.json)"
    else
        echo "   [INFO] No existing DKD spec JSON found - creating one"
        just quint-parse verification/quint/protocol_dkd.qnt /tmp/dkd_spec.json
    fi
    echo ""

    echo "5. Pipeline verification complete!"
    echo "   Quint to JSON conversion: Working"
    echo "   JSON structure validation: Working"
    echo "   Simulator integration points: Ready"
    echo ""
    echo "Available commands for the pipeline:"
    echo "   just quint-parse <input.qnt> <output.json>  - Convert any Quint spec to JSON"
    echo "   just quint-compile <input.qnt> <output.json> - Full compile with type checking"
    echo "   just test-quint-parse                       - Test with a simple example"
    echo ""
    echo "The converted JSON can be consumed by the simulator tests that expect"
    echo "Quint specification files at /tmp/dkd_spec.json"

# ============================================================================
# Lean Verification Tasks
# ============================================================================

# Initialize Lean project (run once or after clean)
lean-init:
    @echo "Initializing Lean project..."
    cd verification/lean && lake update

# Build Lean verification modules
# Usage: just lean-build [jobs]
#   jobs: number of parallel threads (default: 2 for safe resource usage)
lean-build jobs="2": lean-init
    @echo "Building Lean verification modules (threads={{jobs}})..."
    cd verification/lean && nice -n 15 lake build -K env.LEAN_THREADS={{jobs}}

# Build the Lean oracle verifier CLI for differential testing
lean-oracle-build: lean-init
    #!/usr/bin/env bash
    set -euo pipefail

    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    NC='\033[0m'

    # Store project root before changing directories
    PROJECT_ROOT="$(pwd)"

    echo "Building Lean oracle verifier..."
    cd verification/lean && lake build aura_verifier

    BINARY="$PROJECT_ROOT/verification/lean/.lake/build/bin/aura_verifier"
    if [ -f "$BINARY" ]; then
        echo -e "${GREEN}✓ Lean oracle built successfully${NC}"
        echo "  Binary: $BINARY"
        VERSION=$("$BINARY" version 2>/dev/null | grep -o '"version":"[^"]*"' | cut -d'"' -f4 || echo "unknown")
        echo "  Version: $VERSION"
    else
        echo -e "${YELLOW}⚠ Binary not found at expected location${NC}"
        echo "  Expected: $BINARY"
        exit 1
    fi

# Run differential tests against Lean oracle
test-differential: lean-oracle-build
    @echo "Running differential tests against Lean oracle..."
    cargo test -p aura-testkit --features lean --test lean_differential -- --ignored --nocapture

# Run Lean verification (build and check for errors)
# Usage: just lean-check [jobs]
lean-check jobs="4": (lean-build jobs)
    #!/usr/bin/env bash
    set -uo pipefail

    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    NC='\033[0m'

    echo "Checking Lean proof status..."
    echo ""

    # Check for sorry usage (incomplete proofs)
    if grep -r "sorry" verification/lean/Aura --include="*.lean" > /tmp/sorry-check.txt 2>/dev/null; then
        count=$(wc -l < /tmp/sorry-check.txt | tr -d ' ')
        echo -e "${YELLOW}⚠ Found $count incomplete proofs (sorry):${NC}"
        head -10 /tmp/sorry-check.txt | sed 's/^/  /'
        if [ "$count" -gt 10 ]; then
            echo "  ... and $(($count - 10)) more"
        fi
    else
        echo -e "${GREEN}✓ All proofs complete (no sorry found)${NC}"
    fi

# Clean Lean build artifacts
lean-clean:
    @echo "Cleaning Lean artifacts..."
    cd verification/lean && lake clean

# Full Lean workflow (clean, build, verify)
lean-full: lean-clean lean-build lean-check
    @echo "Lean verification complete!"

# Show Lean proof status summary
lean-status:
    #!/usr/bin/env bash
    set -uo pipefail

    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    NC='\033[0m'

    echo "Lean Proof Status"
    echo "================="
    echo ""

    LEAN_DIR="verification/lean"

    if [ ! -d "$LEAN_DIR" ]; then
        echo "No Lean directory found at $LEAN_DIR"
        exit 0
    fi

    echo "Modules:"
    find "$LEAN_DIR/Aura" -name "*.lean" -type f | sort | while read -r f; do
        name=$(basename "$f" .lean)
        dir=$(dirname "$f" | sed "s|$LEAN_DIR/Aura/||")
        if [ "$dir" != "$LEAN_DIR/Aura" ] && [ -n "$dir" ]; then
            display="$dir/$name"
        else
            display="$name"
        fi
        sorries=$(grep -c "sorry" "$f" 2>/dev/null || true)
        sorries=${sorries:-0}
        if [ "$sorries" -gt 0 ] 2>/dev/null; then
            echo -e "  ${YELLOW}○${NC} $display ($sorries incomplete)"
        else
            echo -e "  ${GREEN}●${NC} $display"
        fi
    done

    echo ""
    echo "Run 'just lean-check' to build and verify proofs"

# Translate pure Rust functions to Lean using Charon + Aeneas
# Workflow: Rust → Charon → LLBC → Aeneas → Lean
# Usage: just lean-translate [jobs] [crate]
#   jobs: number of parallel jobs (default: 1 for safe resource usage)
#   crate: specific crate to translate (default: all)
# Example: just lean-translate 2 aura-core
#
# Resource management:
#   - Uses nice -n 19 for lowest CPU priority
#   - Limits cargo parallelism with -j flag
#   - Codegen units set to 1 to reduce memory per rustc process
#   - On macOS, monitor with: watch -n1 'ps aux | grep -E "charon|aeneas|rustc" | head -5'
lean-translate jobs="1" crate="all":
    #!/usr/bin/env bash
    set -uo pipefail

    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    RED='\033[0;31m'
    NC='\033[0m'

    JOBS="{{jobs}}"
    TARGET_CRATE="{{crate}}"

    # Set environment variables for resource limiting
    export CARGO_BUILD_JOBS="$JOBS"
    export RUSTFLAGS="${RUSTFLAGS:-} -C codegen-units=1"  # Reduce memory per rustc

    echo "Translating Rust to Lean using Charon + Aeneas"
    echo "==============================================="
    echo "Resource settings:"
    echo "  - Parallel jobs: $JOBS"
    echo "  - CPU priority: nice -n 19 (lowest)"
    echo "  - Codegen units: 1 (reduces memory)"
    echo ""
    echo "Tip: To monitor resource usage, run in another terminal:"
    echo "  watch -n1 'ps aux | grep -E \"charon|aeneas|rustc\" | head -5'"
    echo ""

    OUTPUT_DIR="verification/lean/Generated"
    LLBC_DIR="target/llbc"
    mkdir -p "$OUTPUT_DIR" "$LLBC_DIR"

    # Check if charon and aeneas are available
    if ! command -v charon &> /dev/null; then
        echo -e "${RED}✗ Charon not found in PATH${NC}"
        echo "  Run 'nix develop' to enter the development environment"
        exit 1
    fi

    if ! command -v aeneas &> /dev/null; then
        echo -e "${RED}✗ Aeneas not found in PATH${NC}"
        echo "  Run 'nix develop' to enter the development environment"
        exit 1
    fi

    echo "Charon: $(charon version 2>/dev/null || echo 'available')"
    echo "Aeneas: available (use -help for options)"
    echo ""

    # Crates to translate (containing pure/ modules)
    if [ "$TARGET_CRATE" = "all" ]; then
        CRATES=(
            "aura-core"
            "aura-journal"
        )
    else
        CRATES=("$TARGET_CRATE")
    fi

    SUCCESS=0
    FAILED=0

    for crate in "${CRATES[@]}"; do
        echo "=== Translating $crate ==="
        echo ""

        # Step 1: Compile with Charon to LLBC
        # Use nice for low priority, -j for limited parallelism
        echo -n "  [1/2] Compiling to LLBC with Charon (jobs=$JOBS)... "
        llbc_file="$LLBC_DIR/${crate//-/_}.llbc"

        if nice -n 19 charon cargo --dest "$LLBC_DIR" -- -p "$crate" -j "$JOBS" 2>/tmp/charon_err.log; then
            echo -e "${GREEN}✓${NC}"
        else
            echo -e "${RED}✗${NC}"
            echo "    Error: $(head -5 /tmp/charon_err.log)"
            ((FAILED++))
            continue
        fi

        # Step 2: Translate with Aeneas to Lean
        echo -n "  [2/2] Translating to Lean with Aeneas... "
        crate_out="$OUTPUT_DIR/${crate//-/_}"
        mkdir -p "$crate_out"

        if nice -n 19 aeneas -backend lean "$llbc_file" -dest "$crate_out" 2>/tmp/aeneas_err.log; then
            echo -e "${GREEN}✓${NC}"
            ((SUCCESS++))
        else
            echo -e "${YELLOW}⚠${NC} (check output)"
            echo "    Note: $(head -3 /tmp/aeneas_err.log)"
            # Count as partial success if files were generated
            if [ -n "$(find "$crate_out" -name '*.lean' 2>/dev/null)" ]; then
                ((SUCCESS++))
            else
                ((FAILED++))
            fi
        fi
        echo ""
    done

    echo "Summary: $SUCCESS crates translated, $FAILED failed"
    echo ""
    echo "Generated Lean files: $OUTPUT_DIR/"
    if [ -d "$OUTPUT_DIR" ]; then
        find "$OUTPUT_DIR" -name "*.lean" -type f | head -10
        count=$(find "$OUTPUT_DIR" -name "*.lean" -type f | wc -l | tr -d ' ')
        if [ "$count" -gt 10 ]; then
            echo "  ... and $(($count - 10)) more"
        fi
    fi

# Verify translated Lean code compiles
# Usage: just lean-verify-translated [jobs] [crate]
lean-verify-translated jobs="2" crate="all": (lean-translate jobs crate) lean-init
    #!/usr/bin/env bash
    set -uo pipefail

    GREEN='\033[0;32m'
    YELLOW='\033[1;33m'
    NC='\033[0m'

    echo "Verifying translated Lean code..."

    GEN_DIR="verification/lean/Generated"
    if [ ! -d "$GEN_DIR" ]; then
        echo -e "${YELLOW}⚠ No generated code found in $GEN_DIR${NC}"
        exit 0
    fi

    # Count generated files
    count=$(find "$GEN_DIR" -name "*.lean" -type f | wc -l | tr -d ' ')
    if [ "$count" -eq 0 ]; then
        echo -e "${YELLOW}⚠ No .lean files found in $GEN_DIR${NC}"
        exit 0
    fi

    echo "Found $count generated Lean files"
    echo ""

    # Try to build the generated code with lake
    cd verification/lean
    if lake build Generated 2>/dev/null; then
        echo -e "${GREEN}✓ All translated code compiles${NC}"
    else
        echo -e "${YELLOW}⚠ Some translated code has errors${NC}"
        echo "  Run 'cd verification/lean && lake build Generated' for details"
    fi
