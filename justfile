# Justfile for Aura project automation
#
# Run `just` or `just --list` to see available commands
# ═══════════════════════════════════════════════════════════════════════════════
# Configuration
# ═══════════════════════════════════════════════════════════════════════════════
# Expected CI Rust version (update when GitHub CI updates)

CI_RUST_VERSION := "1.92"
SCENARIO_CLI := "cargo run --bin aura -- scenarios"
SCENARIO_DIR := "scenarios"

# Default recipe: show available commands
default:
    @just --list

# ═══════════════════════════════════════════════════════════════════════════════
# Internal Helpers
# ═══════════════════════════════════════════════════════════════════════════════

_nix-dev *ARGS:
    nix develop --command {{ ARGS }}

_nix-nightly *ARGS:
    nix develop .#nightly --command {{ ARGS }}

_harness action *ARGS:
    scripts/harness/cmd.sh {{ action }} {{ ARGS }}

# ═══════════════════════════════════════════════════════════════════════════════
# Build
# ═══════════════════════════════════════════════════════════════════════════════

# Build all crates
build:
    cargo build --workspace -q

# Build in release mode
build-release:
    cargo build --workspace --release -q

# Build Aura terminal in development mode (release profile + dev features)
build-dev:
    cargo build -p aura-terminal --bin aura --features development --release
    mkdir -p bin
    cp target/release/aura bin/aura
    @echo "Binary available at: ./bin/aura"

# Build Aura terminal in release mode without dev features
build-terminal-release:
    cargo build -p aura-terminal --bin aura --release --no-default-features --features terminal
    mkdir -p bin
    cp target/release/aura bin/aura
    @echo "Binary available at: ./bin/aura"

# Build app-host binary
build-app-host:
    cargo build -p aura-app --bin app-host --features host --release

# ═══════════════════════════════════════════════════════════════════════════════
# Web App
# ═══════════════════════════════════════════════════════════════════════════════

# Check Aura web shell + shared UI core for the WASM target
web-check:
    CARGO_INCREMENTAL=0 RUSTFLAGS="-C debuginfo=0" cargo check -p aura-ui
    CARGO_INCREMENTAL=0 RUSTFLAGS="-C debuginfo=0" cargo check -p aura-web --target wasm32-unknown-unknown --features web

# Rebuild the local Tailwind bundle used by aura-web (no CDN)
web-tailwind-build:
    cd crates/aura-web && npm ci && npm run tailwind:build

# Watch and rebuild the local Tailwind bundle for aura-web
web-tailwind-watch:
    cd crates/aura-web && npm run tailwind:watch

# Serve Aura web shell locally for harness/browser runs
web-serve port="4173":
    #!/usr/bin/env bash
    set -euo pipefail
    selected_port="{{ port }}"
    if command -v lsof >/dev/null 2>&1; then
        mapfile -t existing_pids < <(lsof -PiTCP:"$selected_port" -sTCP:LISTEN -t 2>/dev/null || true)
        if [ "${#existing_pids[@]}" -gt 0 ]; then
            echo "Port $selected_port is already in use; stopping existing listener(s)." >&2
            lsof -nP -iTCP:"$selected_port" -sTCP:LISTEN >&2 || true
            kill "${existing_pids[@]}" 2>/dev/null || true
            for _ in $(seq 1 30); do
                if ! lsof -PiTCP:"$selected_port" -sTCP:LISTEN -t >/dev/null 2>&1; then
                    break
                fi
                sleep 0.1
            done
            if lsof -PiTCP:"$selected_port" -sTCP:LISTEN -t >/dev/null 2>&1; then
                echo "Port $selected_port is still busy after SIGTERM; force stopping listener(s)." >&2
                kill -9 "${existing_pids[@]}" 2>/dev/null || true
                sleep 0.1
            fi
            if lsof -PiTCP:"$selected_port" -sTCP:LISTEN -t >/dev/null 2>&1; then
                echo "Failed to clear port $selected_port." >&2
                lsof -nP -iTCP:"$selected_port" -sTCP:LISTEN >&2 || true
                exit 1
            fi
        fi
    fi
    echo "Serving aura-web on http://127.0.0.1:$selected_port"
    cd crates/aura-web
    mkdir -p ../../artifacts
    if [ ! -d node_modules ]; then
        npm ci
    fi
    npm run tailwind:build
    # Symlink CSS to target dir so tailwind:watch changes are immediately visible.
    # dx serve copies assets at build time but doesn't re-copy on CSS changes.
    target_css_dir="../../target/dx/aura-web/debug/web/public/assets"
    source_css="$(pwd)/public/assets/tailwind.css"
    sync_tailwind_link() {
        mkdir -p "$target_css_dir"
        rm -f "$target_css_dir/tailwind.css"
        ln -s "$source_css" "$target_css_dir/tailwind.css"
    }
    sync_tailwind_link
    npm run tailwind:watch > ../../artifacts/aura-web-tailwind.log 2>&1 &
    tailwind_pid=$!
    while true; do
        sync_tailwind_link
        sleep 1
    done &
    tailwind_link_pid=$!
    cleanup() {
        kill "$tailwind_pid" 2>/dev/null || true
        kill "$tailwind_link_pid" 2>/dev/null || true
        # Restore terminal settings in case dx serve corrupted them
        stty sane 2>/dev/null || true
    }
    trap cleanup EXIT INT TERM
    NO_COLOR=true ../../scripts/web/dx.sh serve --web --package aura-web --bin aura-web --features web --addr 0.0.0.0 --port "$selected_port" --open false
    # Extra safety: restore terminal after dx exits
    stty sane 2>/dev/null || true

# Alias for the main web development workflow (`web-serve`)
web-dev port="4173":
    just web-serve {{ port }}

web-static port="4173":
    ./scripts/web/serve-static.sh "{{ port }}"

# ═══════════════════════════════════════════════════════════════════════════════
# Browser Harness Shortcuts
# ═══════════════════════════════════════════════════════════════════════════════

# Browser harness driver smoke test
browser-driver-smoke:
    cd crates/aura-harness/playwright-driver && npm test

ci-harness-browser-driver-types:
    bash scripts/check/harness-browser-driver-types.sh

harness-browser-install-check:
    bash scripts/check/harness-browser-install.sh

# Run harness scenario against browser backend config
harness-run-browser scenario config="configs/harness/browser-loopback.toml" artifacts_dir="artifacts/harness/browser":
    just harness-run -- --config {{ config }} --scenario {{ scenario }} --artifacts-dir {{ artifacts_dir }}

# Lint harness run/scenario files for browser backend workflows
harness-lint-browser scenario config="configs/harness/browser-loopback.toml":
    just harness-lint -- --config {{ config }} --scenario {{ scenario }}

harness-boundary-check:
    bash scripts/check/harness-boundary-policy.sh

harness-command-plane-boundary-check:
    bash scripts/check/harness-command-plane-boundary.sh

harness-scenario-inventory-check:
    bash scripts/check/harness-scenario-inventory.sh

harness-shared-scenario-contract-check:
    bash scripts/check/harness-shared-scenario-contract.sh

harness-scenario-legality-check:
    bash scripts/check/harness-scenario-legality.sh

harness-ui-state-evented-check:
    bash scripts/check/harness-ui-state-evented.sh

harness-flake-metrics root="artifacts/harness":
    bash scripts/check/harness-flake-metrics.sh {{ root }}

harness-matrix lane="all" *ARGS:
    bash scripts/harness/run-matrix.sh --lane {{ lane }} {{ ARGS }}

harness-matrix-tui *ARGS:
    just harness-matrix tui {{ ARGS }}

harness-matrix-web *ARGS:
    just harness-matrix web {{ ARGS }}

harness-matrix-all *ARGS:
    just harness-matrix all {{ ARGS }}

harness-shared-semantic-lane lane="all" *ARGS:
    bash scripts/harness/run-matrix.sh --lane {{ lane }} --suite shared {{ ARGS }}

harness-shared-semantic-tui *ARGS:
    just harness-shared-semantic-lane tui {{ ARGS }}

harness-shared-semantic-web *ARGS:
    just harness-shared-semantic-lane web {{ ARGS }}

harness-shared-semantic-matrix *ARGS:
    just harness-shared-semantic-lane all {{ ARGS }}

harness-frontend-conformance-lane lane="all" *ARGS:
    bash scripts/harness/run-matrix.sh --lane {{ lane }} --suite conformance {{ ARGS }}

harness-frontend-conformance-tui *ARGS:
    just harness-frontend-conformance-lane tui {{ ARGS }}

harness-frontend-conformance-web *ARGS:
    just harness-frontend-conformance-lane web {{ ARGS }}

harness-frontend-conformance-matrix *ARGS:
    just harness-frontend-conformance-lane all {{ ARGS }}

ci-shared-flow-policy:
    bash scripts/check/shared-flow-policy.sh

ci-harness-command-plane-boundary:
    bash scripts/check/harness-command-plane-boundary.sh

ci-harness-scenario-shape-contract:
    bash scripts/check/harness-scenario-shape-contract.sh

ci-harness-runtime-events-authoritative:
    bash scripts/check/harness-runtime-events-authoritative.sh

ci-harness-browser-observation-recovery:
    bash scripts/check/harness-browser-observation-recovery.sh

ci-harness-tui-observation-channel:
    bash scripts/check/harness-tui-observation-channel.sh

ci-ui-parity-contract:
    bash scripts/check/ui-parity-contract.sh

quint-observation-scenario:
    ./scripts/verify/quint-observation-scenario.sh

# Replay latest browser harness bundle
harness-replay-browser bundle="artifacts/harness/browser/harness/browser-loopback-smoke/replay_bundle.json":
    just harness-replay -- --bundle {{ bundle }}

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
    cargo test -p {{ crate }} -q

# Run tests for a specific crate in isolation (lib + unit tests only)
test-crate-isolated crate:
    #!/usr/bin/env bash
    echo "Testing {{ crate }} in isolation (lib + unit tests only)..."
    cd "crates/{{ crate }}" && cargo test --lib --verbose

# Benchmark Telltale VM cooperative vs threaded backends for Category C shapes
bench-choreo-parity:
    cargo bench -p aura-agent --features choreo-backend-telltale-vm --bench telltale_vm_backends

# ═══════════════════════════════════════════════════════════════════════════════
# Linting & Formatting
# ═══════════════════════════════════════════════════════════════════════════════

# Check code without building
check:
    cargo check --workspace -q

# Detect legacy authority/device UUID coercions
check-device-id-legacy:
    bash scripts/check/device-id-legacy.sh

audit-device-id-separation:
    bash scripts/check/device-id-legacy.sh audit-live

audit-runtime-device-id-separation:
    bash scripts/check/device-id-legacy.sh audit-runtime

check-bootstrap-guardrails:
    bash scripts/check/bootstrap-guardrails.sh

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
    scripts/check/arch.sh {{ FLAGS }}

# Check invariant-focused lanes (docs + runtime property monitor)
check-invariants:
    scripts/check/arch.sh --invariants
    just ci-property-monitor

# Quick architecture checks by lane

# Example: just check-arch-lane layers
check-arch-lane lane:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{ lane }}" in
      layers|effects|deps|completeness|todos|concurrency|invariants|workflows)
        scripts/check/arch.sh --{{ lane }} || true
        ;;
      *)
        echo "Unknown lane: {{ lane }}"
        echo "Valid lanes: layers, effects, deps, completeness, todos, concurrency, invariants, workflows"
        exit 2
        ;;
    esac

# ═══════════════════════════════════════════════════════════════════════════════
# CI Internal Recipes (called by ci-dry-run and GitHub workflows)
# ═══════════════════════════════════════════════════════════════════════════════

# Build documentation book
ci-book: summary
    AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command bash -c \
        'mdbook-mermaid install . > /dev/null 2>&1 || true && mdbook build && rm -f mermaid-init.js mermaid.min.js'

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

# Harness build check
ci-harness-build:
    cargo build -p aura-harness -q

# Harness contract tests
ci-harness-contract:
    cargo test -p aura-harness --test contract_local_loopback -q
    cargo test -p aura-harness --test contract_suite -q

# Harness browser UiSnapshot evented policy
ci-harness-ui-state-evented:
    bash scripts/check/harness-ui-state-evented.sh

ci-harness-matrix-inventory:
    bash scripts/check/harness-matrix-inventory.sh

# Harness shared intent-contract policy
ci-harness-shared-intent-contract:
    bash scripts/check/harness-shared-scenario-contract.sh
    cargo test -p aura-app shared_intent_contract_accepts_intents --quiet
    cargo test -p aura-app shared_intent_contract_rejects_ui_actions --quiet

# Harness replay regression (nightly mixed-topology lane)
ci-harness-replay:
    cargo build -p aura-terminal --bin aura -q
    AURA_HARNESS_AURA_BIN="$PWD/target/debug/aura" cargo run -p aura-harness --bin aura-harness -- run --config configs/harness/local-loopback.toml --scenario scenarios/harness/semantic-observation-tui-smoke.toml
    AURA_HARNESS_AURA_BIN="$PWD/target/debug/aura" cargo run -p aura-harness --bin aura-harness -- replay --bundle artifacts/harness/local-loopback-smoke/replay_bundle.json

# Browser harness lane (WASM build + Playwright smoke + browser scenarios)
ci-harness-browser:
    ./scripts/ci/harness-browser.sh

ci-harness-matrix-tui:
    ./scripts/ci/harness-matrix-tui.sh

ci-harness-matrix-web:
    ./scripts/ci/harness-matrix-web.sh

ci-harness-matrix:
    just ci-harness-matrix-tui
    just ci-harness-matrix-web

ci-harness-shared-semantic-tui:
    ./scripts/ci/harness-shared-semantic-tui.sh

ci-harness-shared-semantic-web:
    ./scripts/ci/harness-shared-semantic-web.sh

ci-harness-shared-semantic-matrix:
    just ci-harness-shared-semantic-tui
    just ci-harness-shared-semantic-web

ci-harness-frontend-conformance-tui:
    ./scripts/ci/harness-frontend-conformance-tui.sh

ci-harness-frontend-conformance-web:
    ./scripts/ci/harness-frontend-conformance-web.sh

ci-harness-frontend-conformance-matrix:
    just ci-harness-frontend-conformance-tui
    just ci-harness-frontend-conformance-web

# LAN smoke lane for workspace-fast coverage
ci-lan-smoke:
    cargo test -p aura-agent --test lan_integration -q

# LAN deep lane for serialized end-to-end coverage
ci-lan-deep:
    cargo test -p aura-agent --test lan_integration -q -- --ignored

# Test suite (excludes patchbay tests which run in ci-holepunch-tier2)
ci-test:
    cargo test --workspace -- --skip patchbay

# Protocol evolution compatibility gate (async_subtype)
ci-protocol-compat:
    scripts/check/protocol-compat.sh --self-test
    scripts/check/protocol-compat.sh

# Tier 1 deterministic/property tests for holepunch decision logic
ci-holepunch-tier1:
    cargo test -p aura-testkit --test holepunch_tier1 -q

# Tier 2 Patchbay NAT traversal integration scenarios
# Requires: Linux with unprivileged user namespaces enabled, tc, nft in PATH

# CI enables userns via: sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
ci-holepunch-tier2:
    ./scripts/ci/holepunch.sh tier2

# Daily smoke for hole-punch paths with flake tracking output
ci-holepunch-daily-smoke:
    ./scripts/ci/holepunch.sh daily

# Tier 3 nightly stress suite
ci-holepunch-nightly-stress:
    ./scripts/ci/holepunch.sh nightly

# Verify artifact capture and retention outputs exist
ci-holepunch-verify-artifacts:
    ./scripts/ci/holepunch.sh verify-artifacts

# Weekly flaky triage report generation
ci-holepunch-flaky-triage:
    ./scripts/ci/holepunch.sh triage

# Weekly patchbay toolchain audit against pinned fork
ci-holepunch-toolchain-audit:
    ./scripts/ci/holepunch.sh audit

# Property-monitored simulator lane (online property checks + regression helpers)
ci-property-monitor:
    mkdir -p artifacts/property-monitor
    AURA_PROPERTY_MONITOR_REPORT="${PWD}/artifacts/property-monitor/current_report.json" \
    AURA_PROPERTY_MONITOR_BASELINE="${PWD}/crates/aura-simulator/tests/baselines/property_monitor.json" \
    AURA_PROPERTY_MONITOR_TREND="${PWD}/artifacts/property-monitor/trend.json" \
    cargo test -p aura-simulator property_monitor_ci_gate -- --nocapture | tee artifacts/property-monitor/property-monitor.log
    cargo test -p aura-simulator report_comparison_detects_new_violations -q

# Simulator telltale parity lane (artifact-driven differential comparison)
ci-simulator-telltale-parity:
    mkdir -p artifacts/telltale-parity
    AURA_TELLTALE_PARITY_ARTIFACT="${PWD}/artifacts/telltale-parity/report.json" \
    cargo test -p aura-simulator telltale_parity_report_generation_ci -- --nocapture

# Telltale VM parity gates (determinism profile + replay conformance)
ci-choreo-parity:
    mkdir -p artifacts/choreo-parity
    AURA_CONFORMANCE_WRITE_ARTIFACTS=1 \
    AURA_CONFORMANCE_ARTIFACT_DIR="${PWD}/artifacts/choreo-parity" \
    cargo test -p aura-agent --features choreo-backend-telltale-vm --test telltale_vm_parity -q
    cargo test -p aura-agent --features choreo-backend-telltale-vm --lib parity_policy::tests -q

# Choreography concurrency contract gates (link/delegate coherence + canonical fallback)
ci-choreo-concurrency-contracts:
    mkdir -p artifacts/choreo-concurrency-contracts
    AURA_CONFORMANCE_WRITE_ARTIFACTS=1 \
    AURA_CONFORMANCE_ARTIFACT_DIR="${PWD}/artifacts/choreo-concurrency-contracts" \
    cargo test -p aura-agent --features choreo-backend-telltale-vm --test telltale_vm_concurrent_contracts -- --nocapture

# WASM choreography backend matrix for aura-agent

# Note: do not use `--all-features` for aura-agent because choreography backends are exclusive.
ci-agent-wasm:
    cargo check -p aura-agent --target wasm32-unknown-unknown --features web
    cargo check -p aura-agent --target wasm32-unknown-unknown --features "web,choreo-backend-telltale-vm"

# WASM workspace test matrix for crates currently supported on WASM
# Excludes native-only/runtime-heavy crates:
# - aura-terminal: native UI/runtime
# - aura-simulator: native process/simulation host
# - aura-quint: native quint evaluator dependency path
# - aura-agent: test suite currently assumes multi-thread tokio runtime
# - aura-harness: native process/PTY/browser orchestration
# - aura-testkit: native-heavy integration harness and differential suites

# Note: intentionally no `-q` because wasm-bindgen-test-runner rejects forwarded `--quiet`.
ci-workspace-wasm-test:
    #!/usr/bin/env bash
    set -euo pipefail
    : "${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER:=scripts/verify/wasm-bindgen-runner.sh}"
    CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER="$CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER" \
      cargo test --workspace --target wasm32-unknown-unknown \
        --exclude aura-terminal \
        --exclude aura-simulator \
        --exclude aura-quint \
        --exclude aura-agent \
        --exclude aura-harness \
        --exclude aura-testkit

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

# Verify docs links referenced from crates/ resolve to existing files in docs/
ci-crates-doc-links:
    scripts/check/docs-links.sh

# Verify markdown links within docs/ using the same config as GitHub docs workflow
ci-docs-links:
    #!/usr/bin/env bash
    set -euo pipefail
    while IFS= read -r -d '' file; do
        markdown-link-check "$file" -c .github/config/markdown-link-check.json
    done < <(find docs -type f -name '*.md' -print0)

# Verify prose formatting rules used by the docs workflow
ci-text-formatting:
    scripts/check/text-formatting.sh

# Detect semantic drift in documentation (stale type/trait/command references)
ci-docs-semantic-drift:
    scripts/check/docs-semantic-drift.sh

# Verify docs/998_verification_coverage.md metrics match actual codebase
ci-verification-coverage:
    scripts/check/verification-coverage.sh

# Verify user flow coverage mapping remains aligned with changed flow surfaces
ci-user-flow-coverage:
    scripts/check/user-flow-coverage.sh

# Verify shared user-flow policy guardrails and required docs/guidance sync
ci-user-flow-policy:
    scripts/check/user-flow-policy-guardrails.sh
    scripts/check/user-flow-guidance-sync.sh
    just ci-harness-ownership-policy

ci-harness-ownership-policy:
    bash scripts/check/harness-ownership-category-contract.sh
    bash scripts/check/harness-actor-vs-move-ownership.sh
    bash scripts/check/harness-semantic-lifecycle-ownership.sh
    bash scripts/check/harness-readiness-ownership.sh
    bash scripts/check/harness-typed-semantic-errors.sh
    bash scripts/check/harness-move-ownership-boundary.sh
    bash scripts/check/harness-authoritative-fact-boundary.sh

ci-ownership-categories:
    bash scripts/check/ownership-category-declarations.sh

ci-actor-lifecycle:
    bash scripts/check/actor-owned-task-spawn.sh

ci-move-semantics:
    bash scripts/check/move-owned-transfer-only.sh

ci-authoritative-fact-boundary:
    bash scripts/check/authoritative-fact-authorship.sh

ci-capability-boundaries:
    bash scripts/check/capability-gated-mutation.sh

ci-typed-errors:
    bash scripts/check/typed-error-boundary.sh

ci-operation-terminality:
    bash scripts/check/operation-terminality.sh

ci-observed-layer-boundaries:
    bash scripts/check/observed-layer-authorship.sh

ci-timeout-policy:
    bash scripts/check/timeout-policy-boundary.sh

ci-timeout-time-domains:
    bash scripts/check/timeout-time-domain-usage.sh

ci-timeout-backoff:
    bash scripts/check/timeout-backoff-discipline.sh

# Choreography wiring lint
ci-choreo:
    scripts/check/choreo-wiring.sh

ci-async-task-ownership:
    bash scripts/check/async-task-ownership.sh

ci-async-session-ownership:
    bash scripts/check/async-session-ownership.sh

ci-async-concurrency-envelope:
    bash scripts/check/async-concurrency-envelope.sh

ci-runtime-service-lifecycle:
    bash scripts/check/runtime-service-lifecycle.sh

ci-runtime-shutdown-order:
    bash scripts/check/runtime-shutdown-order.sh

ci-async-service-actor-ownership:
    bash scripts/check/async-service-actor-ownership.sh

ci-runtime-instrumentation-schema:
    bash scripts/check/runtime-instrumentation-schema.sh

ci-harness-semantic-lifecycle-ownership:
    bash scripts/check/harness-semantic-lifecycle-ownership.sh

ci-harness-readiness-ownership:
    bash scripts/check/harness-readiness-ownership.sh

ci-harness-typed-semantic-errors:
    bash scripts/check/harness-typed-semantic-errors.sh

ci-harness-move-ownership-boundary:
    bash scripts/check/harness-move-ownership-boundary.sh

ci-harness-authoritative-fact-boundary:
    bash scripts/check/harness-authoritative-fact-boundary.sh

ci-harness-actor-vs-move-ownership:
    bash scripts/check/harness-actor-vs-move-ownership.sh

ci-harness-ownership-category-contract:
    bash scripts/check/harness-ownership-category-contract.sh

# Quint typecheck
ci-quint-typecheck:
    just quint check

# Quint model checking
ci-quint-verify:
    just quint models

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
    trace_file="${PWD}/artifacts/traces/consensus.itf.json"
    trace_dir="${PWD}/artifacts/traces/consensus"
    mkdir -p "$trace_dir"
    echo "Generating ITF traces..."
    quint run --out-itf="$trace_file" verification/quint/consensus/core.qnt --max-steps=30 --max-samples=5
    cp "$trace_file" "$trace_dir/trace.itf.json"
    echo "Running ITF conformance tests..."
    AURA_CONSENSUS_ITF_TRACE="$trace_file" \
      AURA_CONSENSUS_ITF_TRACE_DIR="$trace_dir" \
      AURA_CONFORMANCE_ITF_TRACE="$trace_file" \
      cargo test -p aura-testkit --test consensus_itf_conformance -- --nocapture

# Differential tests
ci-conformance-diff:
    cargo test -p aura-testkit --test consensus_differential -- --nocapture

# Strict native/WASM parity and threaded differential lane
ci-conformance-strict:
    #!/usr/bin/env bash
    set -euo pipefail
    export AURA_CONFORMANCE_WRITE_ARTIFACTS="${AURA_CONFORMANCE_WRITE_ARTIFACTS:-1}"
    export AURA_CONFORMANCE_ARTIFACT_DIR="${AURA_CONFORMANCE_ARTIFACT_DIR:-artifacts/conformance}"
    export AURA_CONFORMANCE_ROTATING_WINDOW="${AURA_CONFORMANCE_ROTATING_WINDOW:-8}"
    export AURA_CONFORMANCE_ITF_SEED_WINDOW="${AURA_CONFORMANCE_ITF_SEED_WINDOW:-8}"

    echo "Running native/threaded parity lane..."
    cargo test -p aura-agent --features choreo-backend-telltale-vm --test telltale_vm_parity -- --nocapture

    echo "Running strict native/wasm parity lane..."
    : "${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER:=scripts/verify/wasm-bindgen-runner.sh}"
    CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER="$CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER" \
      cargo test -p aura-agent --target wasm32-unknown-unknown \
      --features web,choreo-backend-telltale-vm \
      --test telltale_vm_parity -- --nocapture

# Scenario contract bundles (consensus/sync/recovery/reconfiguration)
ci-conformance-contracts:
    #!/usr/bin/env bash
    set -euo pipefail
    export AURA_CONFORMANCE_ARTIFACT_DIR="${AURA_CONFORMANCE_ARTIFACT_DIR:-artifacts/conformance}"
    export AURA_SCENARIO_CONTRACT_ARTIFACT="${AURA_SCENARIO_CONTRACT_ARTIFACT:-$(pwd)/$AURA_CONFORMANCE_ARTIFACT_DIR/scenario_contracts.json}"
    cargo test -p aura-agent --features choreo-backend-telltale-vm --test telltale_vm_scenario_contracts -- --nocapture

# Policy check: protected-branch CI must keep conformance gate job wired
ci-conformance-policy:
    scripts/check/conformance-gate.sh

# Full conformance gate used by CI protected-branch workflows
ci-conformance: ci-conformance-policy
    #!/usr/bin/env bash
    set -euo pipefail
    just ci-conformance-strict
    just ci-conformance-contracts
    just ci-choreo-concurrency-contracts
    just ci-simulator-telltale-parity
    cargo test -p aura-testkit --test conformance_golden_fixtures -- --nocapture
    just ci-conformance-itf
    just ci-conformance-diff

# Lean/Quint bridge cross-validation lane
ci-lean-quint-bridge:
    #!/usr/bin/env bash
    set -euo pipefail
    ARTIFACT_DIR="${AURA_LEAN_QUINT_BRIDGE_ARTIFACT_DIR:-${PWD}/artifacts/lean-quint-bridge}"
    mkdir -p "${ARTIFACT_DIR}"
    AURA_LEAN_QUINT_BRIDGE_ARTIFACT_DIR="${ARTIFACT_DIR}" \
      cargo test -p aura-quint bridge_ -- --nocapture | tee "${ARTIFACT_DIR}/bridge.log"
    if [[ ! -f "${ARTIFACT_DIR}/bridge_discrepancy_report.json" ]]; then
      echo "missing bridge discrepancy report artifact"
      exit 1
    fi
    printf '%s\n' \
      '{"schema_version":"aura.lean-quint-bridge.report.v2","status":"ok","suite":"aura-quint bridge cross-validation","sources":["quint_model_check","lean_certificate"],"discrepancy_detection":"enabled","artifacts":{"bridge_discrepancy":"bridge_discrepancy_report.json","telltale_parity":"../telltale-parity/report.json"}}' \
      > "${ARTIFACT_DIR}/report.json"

# ═══════════════════════════════════════════════════════════════════════════════
# CI Dry Run
# ═══════════════════════════════════════════════════════════════════════════════
# Run GitHub CI checks locally.
# profile=pr  -> jobs that run on pull_request
# profile=push -> jobs that run on push (default)

# profile=all -> push jobs plus scheduled/manual lanes
ci-dry-run profile="push":
    #!/usr/bin/env bash
    set -euo pipefail
    export CARGO_INCREMENTAL=0
    export CARGO_TERM_COLOR=always
    export RUST_BACKTRACE=1
    export AURA_SUPPRESS_NIX_WELCOME=1
    mkdir -p "${PWD}/.tmp"
    export TMPDIR="${PWD}/.tmp"
    GREEN='\033[0;32m' RED='\033[0;31m' YELLOW='\033[0;33m' BLUE='\033[0;34m' NC='\033[0m'
    exit_code=0
    current=0
    STEPS=()
    profile="{{ profile }}"

    add_step() {
        local name="$1" cmd="$2"
        STEPS+=("${name}:::${cmd}")
    }

    run_step() {
        local name="$1" cmd="$2"
        current=$((current + 1))
        printf "[%d/%d] %s... " "$current" "$total" "$name"
        if bash -lc "$cmd" >/dev/null 2>&1; then
            echo -e "${GREEN}OK${NC}"
        else
            echo -e "${RED}FAIL${NC}"
            exit_code=1
        fi
    }

    case "$profile" in
        pr|push|all)
            ;;
        *)
            echo "Unknown ci-dry-run profile: $profile"
            echo "Valid profiles: pr, push, all"
            exit 2
            ;;
    esac

    # CI / Fast (push + pull_request)
    add_step "Format Check"               "nix develop --command just ci-format"
    add_step "Clippy Check"               "nix develop --command just ci-clippy"
    add_step "Build Check"                "nix develop --command just ci-build"
    add_step "Architecture Check"         "nix develop --command scripts/check/arch.sh --quick"
    add_step "User Flow Coverage"         "nix develop --command just ci-user-flow-coverage"
    add_step "User Flow Policy"           "nix develop --command just ci-user-flow-policy"
    add_step "Shared Flow Policy"         "nix develop --command just ci-shared-flow-policy"
    add_step "Tests + Protocol Compat"    "nix develop --command bash -lc 'just ci-test && just ci-protocol-compat'"

    # Docs / Site (push + pull_request)
    add_step "Docs Links"                 "nix develop --command just ci-docs-links"
    add_step "Crate Doc Links"            "nix develop --command just ci-crates-doc-links"
    add_step "Text Formatting Check"      "nix develop --command just ci-text-formatting"
    add_step "Semantic Drift Check"       "nix develop --command just ci-docs-semantic-drift"
    add_step "Docs Build"                 "nix develop --command just ci-book"

    # CI / Deep Conformance (push + pull_request)
    add_step "Conformance Suite"          "nix develop --command bash -lc 'just ci-conformance-policy && CARGO_BUILD_JOBS=4 AURA_CONFORMANCE_WRITE_ARTIFACTS=1 AURA_CONFORMANCE_ARTIFACT_DIR=artifacts/conformance AURA_CONFORMANCE_ROTATING_WINDOW=8 AURA_CONFORMANCE_ITF_SEED_WINDOW=8 just ci-conformance'"
    add_step "Conformance ITF"            "nix develop --command just ci-conformance-itf"
    add_step "Conformance Differential"   "nix develop --command bash -lc 'just lean-oracle-build && just ci-conformance-diff'"
    add_step "Choreography Parity"        "nix develop .#ci --command bash -lc 'CARGO_BUILD_JOBS=4 just ci-choreo-parity'"
    add_step "Property Monitor Suite"     "nix develop .#ci --command just ci-property-monitor"
    add_step "Effects Gate"               "nix develop .#ci --command just ci-effects"
    add_step "Agent WASM Gate"            "nix develop .#ci --command bash -lc 'CARGO_BUILD_JOBS=4 just ci-agent-wasm'"

    if [[ "$profile" != "pr" ]]; then
        # CI / Deep Harness (push)
        add_step "Harness Build"             "nix develop --command just ci-harness-build"
        add_step "Harness Contract"          "nix develop --command just ci-harness-contract"
        add_step "Harness UI Evented"        "nix develop --command just ci-harness-ui-state-evented"
        add_step "Harness Matrix Inventory"  "nix develop --command just ci-harness-matrix-inventory"
        add_step "Harness Shared Intent"     "nix develop --command just ci-harness-shared-intent-contract"
        add_step "Harness Browser"           "nix develop --command just ci-harness-browser"

        # CI / Deep LAN (push)
        add_step "LAN Smoke"                 "nix develop --command just ci-lan-smoke"

        # CI / Deep Verify (push)
        add_step "Lean Proofs"               "nix develop --command bash -lc 'just ci-lean-build && just ci-lean-check-sorry'"
        add_step "Lean-Quint Bridge"         "nix develop --command just ci-lean-quint-bridge"
        add_step "Kani Proofs"               "nix develop .#nightly --command just ci-kani"
    fi

    if [[ "$profile" == "all" ]]; then
        # CI / Deep Harness (schedule + workflow_dispatch)
        add_step "Harness Replay"                    "nix develop --command just ci-harness-replay"
        add_step "Harness Matrix TUI"                "nix develop --command just ci-harness-matrix-tui"
        add_step "Harness Matrix Web"                "nix develop --command just ci-harness-matrix-web"

        # CI / Deep LAN (schedule + workflow_dispatch)
        add_step "LAN Deep"                          "nix develop --command just ci-lan-deep"

        # CI / Scheduled Quint
        add_step "Quint Typecheck"                   "nix develop --command just ci-quint-typecheck"
        add_step "Quint Verification"                "nix develop --command just ci-quint-verify"

        # CI / Scheduled Holepunch
        add_step "Holepunch Daily Smoke"             "nix develop --command just ci-holepunch-daily-smoke"
        add_step "Holepunch Nightly Stress"          "nix develop --command just ci-holepunch-nightly-stress"
        add_step "Holepunch Nightly Artifact Verification" "nix develop --command just ci-holepunch-verify-artifacts"
        add_step "Holepunch Weekly Flaky Triage"     "nix develop --command just ci-holepunch-flaky-triage"
        add_step "Holepunch Weekly Toolchain Audit"  "nix develop --command just ci-holepunch-toolchain-audit"
    fi

    total=${#STEPS[@]}

    echo "CI Dry Run (profile: $profile)"
    echo "==============================="
    echo ""

    # Environment check
    LOCAL_RUST=$(rustc --version | grep -oE '[0-9]+\.[0-9]+' | head -1)
    printf "[0/%d] Rust version... " "$total"
    if [[ "$LOCAL_RUST" == "{{ CI_RUST_VERSION }}" ]]; then
        echo -e "${GREEN}$LOCAL_RUST${NC} (matches CI)"
    elif [[ "$LOCAL_RUST" < "{{ CI_RUST_VERSION }}" ]]; then
        echo -e "${YELLOW}$LOCAL_RUST${NC} (CI uses {{ CI_RUST_VERSION }} - newer lints may fail in CI)"
    else
        echo -e "${BLUE}$LOCAL_RUST${NC} (newer than CI {{ CI_RUST_VERSION }})"
    fi
    echo ""

    for step in "${STEPS[@]}"; do
        name="${step%%:::*}"
        cmd="${step#*:::}"
        run_step "$name" "$cmd"
    done

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
    rm -rf docs/book/ mermaid.min.js mermaid-init.js
    echo "Cleaning logs..."
    rm -rf logs/ *.log
    echo "Cleaning demo/test data..."
    rm -rf .aura-demo/ .aura-test/ outcomes/ *.sealed *.dat *.tmp *.temp
    rm -rf traces/ .qemu-vm/ .patchbay-work/ .patchbay-target/ target-codex/
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

# Clean old compilation artifacts (keeps builds from last 7 days)
sweep:
    cargo sweep --time 7

# Clean all build artifacts except those for installed toolchains
sweep-installed:
    cargo sweep --installed

# Show what would be cleaned without removing anything
sweep-dry-run:
    cargo sweep --time 7 --dry-run

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

# Generate `docs/SUMMARY.md` from Markdown files
summary:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "${TMPDIR:-/tmp}"  # Ensure temp dir exists (nix-shell cleanup)
    docs="docs"; build_dir="$docs/book"; out="$docs/SUMMARY.md"

    get_title() {
        local title="$(grep -m1 '^# ' "$1" | sed 's/^# *//')"
        if [ -z "$title" ]; then
            title="$(basename "${1%.*}" | tr '._-' '   ' | awk '{for(i=1;i<=NF;i++){$i=toupper(substr($i,1,1))substr($i,2)}}1')"
        fi
        echo "$title"
    }

    echo "# Summary" > "$out"; echo "" >> "$out"

    root_count=0
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        echo "- [$(get_title "$f")](${f#$docs/})" >> "$out"
        root_count=$((root_count + 1))
    done < <(find "$docs" -maxdepth 1 -type f -name '*.md' -not -name 'SUMMARY.md' | LC_ALL=C sort)

    while IFS= read -r dir_path; do
        [ -n "$dir_path" ] || continue
        dir="${dir_path#$docs/}"
        has_files=0
        while IFS= read -r f; do
            [ -n "$f" ] || continue
            if [ "$has_files" -eq 0 ]; then
                [ "$root_count" -gt 0 ] && echo "" >> "$out"
                echo "# $(echo "$dir" | tr '_-' '  ' | awk '{for(i=1;i<=NF;i++){$i=toupper(substr($i,1,1))substr($i,2)}}1')" >> "$out"
                echo "" >> "$out"
                has_files=1
                root_count=1
            fi
            echo "- [$(get_title "$f")](${f#$docs/})" >> "$out"
        done < <(find "$dir_path" -type f -name '*.md' | LC_ALL=C sort)
    done < <(find "$docs" -mindepth 1 -maxdepth 1 -type d -not -name 'book' | LC_ALL=C sort)
    echo "Wrote $out"

# Build and serve the docs book with live reload (after regenerating summary)
book: summary
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p "${TMPDIR:-/tmp}"  # Ensure temp dir exists (nix-shell cleanup)
    pgrep -x mdbook > /dev/null && { echo "Stopping existing mdbook server..."; pkill mdbook; sleep 1; }
    trap 'rm -f mermaid-init.js mermaid.min.js' EXIT
    AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command bash -c \
        'mdbook-mermaid install . > /dev/null 2>&1 || true && mdbook serve --open'

# Alias for `book`
serve: book

# Generate rustdoc documentation
docs:
    cargo doc --workspace --no-deps --open

# ═══════════════════════════════════════════════════════════════════════════════
# Quickstart
# ═══════════════════════════════════════════════════════════════════════════════
# Quickstart helper for local onboarding and smoke coverage
# Usage:
#   just quickstart init
#   just quickstart status

# just quickstart smoke
quickstart action="init":
    ./scripts/dev/quickstart.sh "{{ action }}"

# Run macOS Keychain integration tests
test-macos-keychain:
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "$OSTYPE" != "darwin"* ]] && { echo "Error: macOS only"; exit 1; }
    echo "Aura macOS Keychain Integration Tests"
    echo "======================================"
    export RUST_LOG=debug RUST_BACKTRACE=1
    cargo test -p aura-agent --test macos_keychain_tests -- --nocapture --test-threads=1

# ═══════════════════════════════════════════════════════════════════════════════
# Quint Specifications
# ═══════════════════════════════════════════════════════════════════════════════
# Quint local workflows (canonical entry point)
# Usage:
#   just quint setup
#   just quint check

# just quint models
quint mode="check":
    ./scripts/verify/quint-workflow.sh "{{ mode }}"

# Parse Quint file to JSON
quint-parse input output:
    just _nix-dev -- quint parse --out {{ output }} {{ input }}

# Parse and typecheck Quint file
quint-compile input output:
    just _nix-dev -- quint typecheck {{ input }}
    just _nix-dev -- quint parse --out {{ output }} {{ input }}

# Verify a single Quint spec with specific invariants
quint-verify spec invariants:
    #!/usr/bin/env bash
    set -uo pipefail
    mkdir -p verification/quint/traces
    SPEC_NAME=$(basename "{{ spec }}" .qnt)
    INV_FLAGS=""
    IFS=',' read -ra INVS <<< "{{ invariants }}"
    for inv in "${INVS[@]}"; do INV_FLAGS="$INV_FLAGS --invariant=$inv"; done
    quint verify "{{ spec }}" $INV_FLAGS --max-steps=10 \
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
    ./scripts/verify/workflow.sh quint-types {{ verbose }}

# Generate verification coverage report
verification-coverage format="--md":
    ./scripts/verify/workflow.sh coverage {{ format }}

# ═══════════════════════════════════════════════════════════════════════════════
# Quint Semantic Traces
# ═══════════════════════════════════════════════════════════════════════════════

# Regenerate a deterministic Quint semantic trace for harness / simulator workflows
quint-semantic-trace spec="verification/quint/harness/flows.qnt" out="verification/quint/traces/harness_flows.itf.json" seed="424242" max_steps="50":
    QUINT_TRACE_SEED={{ seed }} QUINT_TRACE_MAX_STEPS={{ max_steps }} just _nix-dev -- ./scripts/verify/quint-semantic-trace.sh generate {{ spec }} {{ out }}

# Check that the checked-in Quint semantic trace matches regeneration
quint-semantic-trace-check spec="verification/quint/harness/flows.qnt" expected="verification/quint/traces/harness_flows.itf.json" seed="424242" max_steps="50":
    QUINT_TRACE_SEED={{ seed }} QUINT_TRACE_MAX_STEPS={{ max_steps }} just _nix-dev -- ./scripts/verify/quint-semantic-trace.sh check {{ spec }} {{ expected }}

# ═══════════════════════════════════════════════════════════════════════════════
# Lean Formal Verification
# ═══════════════════════════════════════════════════════════════════════════════

# Initialize Lean project (run once or after clean)
lean-init:
    @echo "Initializing Lean project..."
    cd verification/lean && lake update

# Verify Lean proofs (canonical command)
verify-lean jobs="2": lean-init
    #!/usr/bin/env bash
    set -euo pipefail
    GREEN='\033[0;32m' YELLOW='\033[1;33m' NC='\033[0m'
    echo "Lean Formal Verification"
    echo "========================"
    echo "Building Lean verification modules (threads={{ jobs }})..."
    cd verification/lean && nice -n 15 lake build -K env.LEAN_THREADS={{ jobs }}
    cd ../..
    if grep -r "sorry" verification/lean/Aura --include="*.lean" > /tmp/sorry-check.txt 2>/dev/null; then
        count=$(wc -l < /tmp/sorry-check.txt | tr -d ' ')
        echo -e "${YELLOW}⚠ Found $count incomplete proofs (sorry)${NC}"
        head -10 /tmp/sorry-check.txt | sed 's/^/  /'
    else
        echo -e "${GREEN}✓ All proofs complete${NC}"
    fi

# Backward-compatibility alias (prefer `just verify-lean`)
lean-build jobs="2": (verify-lean jobs)

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
lean-check jobs="4": (verify-lean jobs)

# Clean Lean build artifacts
lean-clean:
    cd verification/lean && lake clean

# Full Lean workflow (clean, build, verify)
lean-full jobs="2": lean-clean (verify-lean jobs)
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
    JOBS="{{ jobs }}"; TARGET="{{ crate }}"
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
    just _nix-nightly -- cargo kani --package {{ package }} --default-unwind {{ unwind }}

# Run a specific Kani harness
kani-harness harness package="aura-protocol" unwind="10":
    just _nix-nightly -- cargo kani --package {{ package }} --harness {{ harness }} --default-unwind {{ unwind }}

# Setup Kani (first time only)
kani-setup:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Setting up Kani verifier..."
    just _nix-nightly -- cargo install --locked kani-verifier
    just _nix-nightly -- cargo kani setup
    echo "✓ Kani setup complete! Run 'just kani' to verify."

# Run full Kani verification suite
kani-suite:
    @./scripts/verify/workflow.sh kani

# ═══════════════════════════════════════════════════════════════════════════════
# Combined Verification
# ═══════════════════════════════════════════════════════════════════════════════

# Consensus conformance tests (matches CI)
verify-conformance:
    #!/usr/bin/env bash
    set -euo pipefail
    GREEN='\033[0;32m' RED='\033[0;31m' NC='\033[0m'
    echo "Consensus Conformance Tests"
    echo "==========================="

    echo "[1/3] Generating ITF traces..."
    trace_file="${PWD}/artifacts/traces/consensus.itf.json"
    trace_dir="${PWD}/artifacts/traces/consensus"
    mkdir -p "$trace_dir"
    quint run --out-itf="$trace_file" verification/quint/consensus/core.qnt \
        --max-steps=30 --max-samples=5 || { echo -e "${RED}[FAIL]${NC}"; exit 1; }
    cp "$trace_file" "$trace_dir/trace.itf.json"
    echo -e "${GREEN}[OK]${NC} Generated traces"

    echo "[2/3] Running ITF conformance tests..."
    AURA_CONSENSUS_ITF_TRACE="$trace_file" \
      AURA_CONSENSUS_ITF_TRACE_DIR="$trace_dir" \
      AURA_CONFORMANCE_ITF_TRACE="$trace_file" \
      cargo test -p aura-protocol --test consensus_itf_conformance -- --nocapture || \
        { echo -e "${RED}[FAIL]${NC}"; exit 1; }
    echo -e "${GREEN}[OK]${NC} Conformance passed"

    echo "[3/3] Running differential tests..."
    cargo test -p aura-protocol --test consensus_differential -- --nocapture || \
        { echo -e "${RED}[FAIL]${NC}"; exit 1; }
    echo -e "${GREEN}[OK]${NC} Differential passed"

# Run all verification (Lean + Quint + Conformance)
verify-all: verify-lean
    just quint models
    just verify-conformance
    @echo ""; echo "ALL VERIFICATION COMPLETE"

# ═══════════════════════════════════════════════════════════════════════════════
# Scenarios
# ═══════════════════════════════════════════════════════════════════════════════
# Unified scenario entry point
# Examples:
#   just scenario run
#   just scenario run --pattern scenario1
#   just scenario list --detailed
#   just scenario validate --strictness standard
#   just scenario discover --validate

# just scenario report --input outcomes/scenario_results.json --output outcomes/scenario_report.html --format html --detailed
scenario subcommand="run" *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{ subcommand }}" in
      run|list|validate)
        {{ SCENARIO_CLI }} "{{ subcommand }}" --directory {{ SCENARIO_DIR }} {{ ARGS }}
        ;;
      discover)
        {{ SCENARIO_CLI }} discover --root . {{ ARGS }}
        ;;
      report)
        {{ SCENARIO_CLI }} report {{ ARGS }}
        ;;
      *)
        echo "Unknown scenario subcommand: {{ subcommand }}"
        echo "Valid subcommands: run, list, validate, discover, report"
        exit 2
        ;;
    esac

# Run full scenario suite and generate an HTML report
scenario-suite:
    #!/usr/bin/env bash
    set -euo pipefail
    just scenario discover --validate
    just scenario validate --strictness standard
    {{ SCENARIO_CLI }} run --directory {{ SCENARIO_DIR }} --output-file outcomes/scenario_results.json --detailed-report
    just scenario report --input outcomes/scenario_results.json --output outcomes/scenario_report.html --format html --detailed
    echo "Report: outcomes/scenario_report.html"

# ═══════════════════════════════════════════════════════════════════════════════
# Runtime Harness
# ═══════════════════════════════════════════════════════════════════════════════

# Run harness coordinator with a TOML run config and optional scenario file
harness-run *ARGS:
    just _harness -- run {{ ARGS }}

# Lint harness TOML run and scenario files
harness-lint *ARGS:
    just _harness -- lint {{ ARGS }}

# Replay a previously recorded harness bundle
harness-replay *ARGS:
    just _harness -- replay {{ ARGS }}

# Run harness crate tests
harness-test:
    cargo test -p aura-harness

# Run LAN integration smoke coverage
lan-test-smoke:
    just ci-lan-smoke

# Run LAN integration deep coverage
lan-test-deep:
    just ci-lan-deep

# ═══════════════════════════════════════════════════════════════════════════════
# Nix / Hermetic Builds
# ═══════════════════════════════════════════════════════════════════════════════

# Generate Cargo.nix using crate2nix
generate-cargo-nix:
    just _nix-dev -- crate2nix generate
    @echo "Run 'nix build .#aura-terminal' to test hermetic build"

# Build using hermetic Nix build
build-nix:
    nix build .#aura-terminal

# Build specific package with hermetic Nix
build-nix-package package:
    nix build .#{{ package }}

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
# WASM DB test helper
# Usage:
#   just wasm-db build
#   just wasm-db serve

# just wasm-db test
wasm-db action="build":
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{ action }}" in
      build)
        cd crates/db-test
        wasm-pack build --target web --out-dir web/pkg
        echo "Output: crates/db-test/web/pkg/"
        ;;
      serve)
        [ -d "crates/db-test/web/pkg" ] || { echo "Run: just wasm-db build"; exit 1; }
        echo "Server: http://localhost:8000"
        cd crates/db-test/web && python3 -m http.server 8000
        ;;
      test)
        just wasm-db build
        just wasm-db serve
        ;;
      *)
        echo "Unknown wasm-db action: {{ action }}"
        echo "Valid actions: build, serve, test"
        exit 2
        ;;
    esac

# ═══════════════════════════════════════════════════════════════════════════════
# Utilities
# ═══════════════════════════════════════════════════════════════════════════════

# Launch the cross-frontend developer demo UX with isolated TUI + web instances.
demo-dual *ARGS='':
    just build-dev
    ./scripts/dev/demo-dual.sh --mode dual {{ ARGS }}

# Launch only the isolated TUI side of the developer demo UX.
demo-tui *ARGS='':
    just build-dev
    ./scripts/dev/demo-dual.sh --mode tui {{ ARGS }}

# Launch only the isolated web side of the developer demo UX with a dedicated browser profile.
demo-web *ARGS='':
    ./scripts/dev/demo-dual.sh --mode web {{ ARGS }}

# Preserve the existing simulated TUI demo under an explicit name.
demo-sim:
    just build-dev
    ./bin/aura tui --demo

# Run the simulated TUI demo with logging to file.
demo-sim-log log="/tmp/aura-demo.log":
    #!/usr/bin/env bash
    set -euo pipefail
    just build-dev
    rm -f {{ log }}
    echo "Starting simulated TUI demo with logging to {{ log }}"
    echo "After exiting, view logs with: grep -i ContactsSignalView {{ log }}"
    RUST_LOG=aura_agent=info,aura_app=info,aura_terminal=info AURA_TUI_ALLOW_STDIO=1 ./bin/aura tui --demo 2>{{ log }}

# Alias the old `just demo` surface to the cross-frontend developer demo UX.
demo *ARGS='':
    just build-dev
    ./scripts/dev/demo-dual.sh --mode dual {{ ARGS }}

# Backwards-compatible alias for the simulated TUI demo log helper.
demo-log log="/tmp/aura-demo.log":
    just demo-sim-log {{ log }}

# Smoke-check the developer demo launcher startup, cleanup, and rerun behavior.
demo-smoke:
    bash scripts/check/demo-dual-smoke.sh

# Execute any aura CLI command
aura *ARGS='--help':
    @AURA_SUPPRESS_NIX_WELCOME=1 nix develop --quiet --command cargo run --bin aura -- {{ ARGS }}

# Show project metrics
metrics:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Project Metrics"
    echo "==============="
    if command -v tokei >/dev/null 2>&1; then
        echo "Code size (tokei):"
        tokei crates
    else
        echo "Code size (fallback line count):"
        find crates -name "*.rs" -type f -exec cat {} + | wc -l
    fi
    echo "Number of crates:"
    ls -1 crates | wc -l

# Install git hooks
install-hooks:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p .githooks
    chmod +x .githooks/pre-commit
    git config core.hooksPath .githooks
    echo "Installed hooks from .githooks (core.hooksPath=.githooks)"
