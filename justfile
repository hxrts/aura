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

# Run clippy linter
clippy:
    cargo clippy --workspace --all-targets --verbose -- -D warnings

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

# Run all checks (format, clippy, test)
ci: fmt-check clippy test
    @echo "All CI checks passed!"

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
    @echo "just fmt-check && just clippy" >> .git/hooks/pre-commit
    @chmod +x .git/hooks/pre-commit
    @echo "Git hooks installed"

