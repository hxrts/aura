# Justfile for Aura project automation

# Default recipe - show available commands
default:
    @just --list

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

# Check code without building
check:
    cargo check --workspace --verbose

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
    cargo run --bin aura -- status -c .aura/config_1.toml

# Test key derivation
# test-dkd app_id context:
#     cargo run --bin aura -- test-dkd --app-id {{app_id}} --context {{context}} -f .aura/config_1.toml
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

    echo "2. Verifying ledger and config files creation..."
    if [ -f ".aura-test/ledger.cbor" ]; then
        echo "OK Ledger file created successfully"
        echo "Ledger size: $(stat -c%s .aura-test/ledger.cbor 2>/dev/null || stat -f%z .aura-test/ledger.cbor) bytes"
    else
        echo "ERROR: Ledger file not found"
        exit 1
    fi
    if [ -f ".aura-test/config_1.toml" ]; then
        echo "OK Config file created successfully"
    else
        echo "ERROR: Config file not found"
        exit 1
    fi
    echo ""

    echo "3. Checking account status..."
    cargo run --bin aura -- status -c .aura-test/config_1.toml
    echo "OK Status retrieved"
    echo ""

    echo "4. Testing scenario discovery..."
    if [ -d "scenarios" ]; then
        cargo run --bin aura -- scenarios discover --root . > /dev/null 2>&1
        echo "OK Scenario discovery functional"
    else
        echo "SKIP No scenarios directory found"
    fi
    echo ""

    echo "Phase 0 smoke tests passed!"
    echo "Full threshold account creation and status checking functional!"

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

# Run all checks with effects enforcement (format, clippy-strict, test)
ci: fmt-check clippy-strict test
    @echo "All CI checks passed with effects enforcement!"

# Run basic CI checks (legacy - use ci for effects enforcement)
ci-basic: fmt-check clippy test
    @echo "Basic CI checks passed!"

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
    nix develop --command quint parse /tmp/simple.qnt > /dev/null && echo "✓ Basic parsing works"
    echo ""
    
    echo "Quint setup verification completed!"

# Execute any aura CLI command with nix build  
# Usage: just aura init -n 3 -t 2 -o test-account
# Usage: just aura status -c test-account/config_1.toml
# Usage: just aura scenarios list
aura *ARGS='--help':
    @AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command cargo build --bin aura
    @AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command cargo run --bin aura -- {{ARGS}}

# Generate documentation
docs:
    cargo doc --workspace --no-deps --open

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
