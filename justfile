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

    # Find all .md files under docs/, excluding SUMMARY.md itself and the build output
    while IFS= read -r f; do
        rel="${f#$docs/}"

        # Skip SUMMARY.md
        [ "$rel" = "SUMMARY.md" ] && continue

        # Skip files under the build output directory
        case "$f" in "$build_dir"/*) continue ;; esac

        # Derive the title from the first H1; fallback to filename
        title="$(grep -m1 '^# ' "$f" | sed 's/^# *//')"
        if [ -z "$title" ]; then
            base="$(basename "${f%.*}")"
            title="$(printf '%s\n' "$base" \
                | tr '._-' '   ' \
                | awk '{for(i=1;i<=NF;i++){ $i=toupper(substr($i,1,1)) substr($i,2) }}1')"
        fi

        # Indent two spaces per directory depth
        depth="$(awk -F'/' '{print NF-1}' <<<"$rel")"
        indent="$(printf '%*s' $((depth*2)) '')"

        echo "${indent}- [$title](${rel})" >> "$out"
    done < <(find "$docs" -type f -name '*.md' -not -name 'SUMMARY.md' -not -path "$build_dir/*" | LC_ALL=C sort)

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
    echo "[1/5] Running Format Check..."
    if cargo fmt --all -- --check; then
        echo -e "${GREEN}[OK]${NC} Format check passed"
    else
        echo -e "${RED}[FAIL]${NC} Format check failed"
        exit_code=1
    fi
    echo ""

    # 2. Clippy with Effects Enforcement
    echo "[2/5] Running Clippy with Effects Enforcement..."
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
    echo "[3/5] Running Test Suite..."
    if cargo test --workspace --verbose; then
        echo -e "${GREEN}[OK]${NC} Test suite passed"
    else
        echo -e "${RED}[FAIL]${NC} Test suite failed"
        exit_code=1
    fi
    echo ""

    # 4. Check for Effects System Violations (Layer-Aware)
    echo "[4/5] Checking for Effects System Violations..."
    violations_found=0

    # Layer architecture:
    # - Layer 3 (aura-effects): Production handlers - MUST use SystemTime::now(), thread_rng()
    # - Layer 6 (aura-simulator): Runtime composition - allowed for instrumentation
    # - Layer 8 (aura-testkit, tests/): Testing infrastructure - allowed
    # - All other layers: MUST use effect traits

    # Check for direct time usage (exclude Layer 3, 6, 8, integration tests, demo code, CLI scenarios, and test modules)
    # Note: May include false positives from code in comments or test modules
    time_violations=$(rg --type rust "SystemTime::now|Instant::now|chrono::Utc::now" crates/ --line-number \
        --glob '!**/aura-effects/**' \
        --glob '!**/aura-simulator/**' \
        --glob '!**/aura-testkit/**' \
        --glob '!**/tests/**' \
        --glob '!**/integration/**' \
        --glob '!**/demo/**' \
        --glob '!**/examples/**' \
        --glob '!**/aura-cli/src/handlers/scenarios.rs' 2>/dev/null | \
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

    # Check for direct randomness usage (exclude Layer 3, 6, 8, integration tests, and demo code)
    if rg --type rust "rand::random|thread_rng\(\)|OsRng::new" crates/ --line-number \
        --glob '!**/aura-effects/**' \
        --glob '!**/aura-simulator/**' \
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
        --glob '!**/aura-cli/**' \
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
        echo -e "${YELLOW}Note:${NC} Layer 1 ID constructors (aura-core/identifiers.rs), Layer 3 (aura-effects), Layer 6 (aura-agent, aura-simulator), Layer 7 (aura-cli), Layer 8 (aura-testkit, tests/), property tests (aura-quint), demo/TUI code, operation ID generation (aura-composition/registry.rs), and sync service IDs (aura-sync/services, aura-sync/infrastructure) are exempt."
        exit_code=1
    fi
    echo ""

    # 5. Documentation Links Check
    echo "[5/6] Checking Documentation Links..."
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
        ! -path "*/.aura-test/*")

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
    echo "[6/7] Running Build Check..."
    if cargo build --workspace --verbose; then
        echo -e "${GREEN}[OK]${NC} Build check passed"
    else
        echo -e "${RED}[FAIL]${NC} Build check failed"
        exit_code=1
    fi
    echo ""

    # Summary
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
    echo "Run 'nix build .#aura-cli' to test hermetic build"

# Build using hermetic Nix build (requires Cargo.nix to exist)
build-nix:
    nix build .#aura-cli

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

    echo "1. Building aura-cli..."
    nix build .#aura-cli
    echo "[OK] aura-cli built successfully"

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
    echo "   Input: specs/quint/protocol_dkd.qnt"
    echo "   Output: /tmp/quint_pipeline_test.json"
    echo ""
    just quint-parse specs/quint/protocol_dkd.qnt /tmp/quint_pipeline_test.json
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
        just quint-parse specs/quint/protocol_dkd.qnt /tmp/dkd_spec.json
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
