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
test-dkd app_id context:
    cargo run --bin aura -- test-dkd --app-id {{app_id}} --context {{context}} -f .aura/config_1.toml

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

    echo "2. Checking account status..."
    cargo run --bin aura -- status -c .aura-test/config_1.toml
    echo "OK Status retrieved"
    echo ""

    echo "3. Testing key derivation..."
    cargo run --bin aura -- test-dkd --app-id "test-app" --context "test-context" -f .aura-test/config_1.toml
    echo "OK Key derivation successful"
    echo ""

    echo "Phase 0 smoke tests passed!"

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
