# Aura Test Suite

This directory contains a comprehensive test suite organized by purpose and scope, aligned with the current AuthorityId-centric architecture.

## Test Organization

### End-to-End Tests (`e2e/`)

Complete system workflows using real effect handlers and current architecture:

- **`01_authority_lifecycle.rs`** - Authority creation, contexts, journal operations
- **`02_threshold_operations.rs`** - FROST signatures, threshold protocols, recovery
- **`04_storage_authorization.rs`** - Biscuit-based storage access and permissions

### Property Tests (`properties/`)

Mathematical and architectural correctness validation:

- **`architecture_compliance.rs`** - 8-layer dependency enforcement  
- **`crdt_convergence.rs`** - CRDT consistency and convergence properties

### CLI Tests (`cli/`)

User interface testing with real CLI commands:

- **`authority_commands.rs`** - Authority management CLI operations

### Integration Tests (`integration/`)

Cross-component interaction testing (planned):

- **`effect_system.rs`** - Effect handler composition
- **`guard_chain.rs`** - Authorization pipeline
- **`transport_protocols.rs`** - Network protocol testing

## Architecture Compliance

All tests in this suite:
- ✅ Use **AuthorityId** (not legacy DeviceId)
- ✅ Integrate with **aura-agent** effect system
- ✅ Follow **8-layer architecture** boundaries  
- ✅ Use **current APIs** (no deprecated functions)
- ✅ Test **real functionality** (minimal mocking)

**Purpose:**
- Quick validation that the system is working
- Scenario TOML parsing and validation
- Basic simulator configuration checks
- Property verification setup
- Structural validation of choreographies

**What Smoke Tests Do:**
1. **Discover** all `.toml` files in the `scenarios/` directory tree
2. **Parse** each scenario file into a structured format
3. **Validate** scenario structure:
   - Threshold doesn't exceed participant count
   - All referenced properties are defined
   - Choreography participants meet threshold requirements
   - Phase actions are well-formed
4. **Report** results: passed, failed, or skipped

**What Smoke Tests DON'T Do:**
- Execute actual choreographies (reserved for full integration tests)
- Simulate network behavior
- Verify cryptographic correctness
- Run property checkers

**Running Smoke Tests:**

```bash
# Run all smoke tests
cargo test --test smoke_test

# Run specific smoke test
cargo test --test smoke_test smoke_test_dkd_basic

# Run all tests with output
cargo test --test smoke_test -- --nocapture

# Run smoke tests in workspace
just test
```

**Individual Test Cases:**

- `smoke_test_all_scenarios()` - Runs validation on all scenario files
- `smoke_test_dkd_basic()` - Tests DKD basic scenario specifically
- `smoke_test_crdt_convergence()` - Tests CRDT convergence scenario
- `smoke_test_threshold_key_generation()` - Tests threshold key generation scenario

**Expected Output:**

```
🔬 Running Smoke Tests
======================
Scenarios directory: /path/to/scenarios
Found 22 scenario files

Testing: core_protocols/dkd_basic.toml
  Scenario: dkd_basic_derivation
  Description: Basic P2P deterministic key derivation scenario
  Participants: 2
  Threshold: 2
  Phases: 4
    Phase 1: handshake (1 actions)
    Phase 2: context_agreement (1 actions)
      Verify: derived_keys_match = true
    Phase 3: key_derivation (1 actions)
    Phase 4: validation (2 actions)
    Properties: 4
      derived_keys_match: Safety
      derivation_deterministic: Safety
      no_key_leakage: Safety
      derivation_completes: Liveness
  PASSED

...

======================
📊 Smoke Test Summary
======================
  Passed:  20
  ❌ Failed:  0
  ⏭️  Skipped: 2
  📝 Total:   22
```

**Common Failure Reasons:**

1. **Invalid threshold configuration** - Threshold exceeds participants
2. **Undefined properties** - Phase verifies property not in properties list
3. **Malformed TOML** - Syntax errors in scenario file
4. **Missing required fields** - Scenario missing metadata or setup sections
5. **Choreography participant mismatch** - Not enough participants for threshold

**Adding New Scenarios:**

To add a new scenario that will be picked up by smoke tests:

1. Create a `.toml` file in the `scenarios/` directory or any subdirectory
2. Follow the scenario structure (see examples in `scenarios/core_protocols/`)
3. Run `cargo test --test smoke_test` to validate

**Scenario File Structure:**

```toml
[metadata]
name = "my_scenario"
description = "What this scenario tests"
version = "1.0.0"
author = "Your Name"
tags = ["tag1", "tag2"]

[setup]
participants = 3
threshold = 2
seed = 42  # Optional: for deterministic randomness

[network]  # Optional
latency_range = [10, 50]  # milliseconds
drop_rate = 0.05  # 5% packet loss

[[phases]]
name = "phase_name"
description = "What this phase does"
timeout_seconds = 5
actions = [
    { type = "run_choreography", choreography = "protocol_name", participants = ["p1", "p2"] },
    { type = "verify_property", property = "safety_property", expected = true },
    { type = "wait_ticks", ticks = 100 },
]

[[properties]]
name = "safety_property"
property_type = "safety"  # or "liveness"
```

### `e2e_choreography_test.rs` - End-to-End Choreography Tests

Tests actual execution of choreographic protocols with multiple participants. These tests go beyond structural validation to verify runtime behavior.

**Coverage:**
- Broadcast-and-gather choreography (3 and 5 participants)
- Threshold collection with 2-of-3 quorum
- Multi-round coordination patterns
- Error handling (timeouts, invalid messages)
- Deterministic effects and reproducibility
- Scenario integration

**Running:**

```bash
# Run all E2E choreography tests
cargo test --test e2e_choreography_test

# Run with detailed output
cargo test --test e2e_choreography_test -- --nocapture

# Run specific test
cargo test --test e2e_choreography_test test_broadcast_gather_3_participants
```

See `E2E_TEST_IMPLEMENTATION.md` for detailed documentation.

### `integration_middleware.rs` - Cross-Layer Middleware Tests

Tests the interaction between middleware from different architectural layers to ensure they compose correctly.

**Coverage:**
- Transport layer middleware (rate limiting, circuit breaking, monitoring, compression)
- Journal layer middleware (validation, authorization, observability, retry)
- Cross-layer error propagation
- Performance characteristics
- Observability and monitoring

**Running:**

```bash
cargo test --test integration_middleware
```

## Test Organization

```
tests/
├── README.md                      # This file
├── smoke_test.rs                  # Scenario-based smoke tests
└── integration_middleware.rs      # Cross-layer middleware tests
```

## Continuous Integration

These integration tests are run as part of the CI pipeline:

```bash
just ci  # Runs format check, clippy, and all tests including integration
```

## Troubleshooting

### Smoke tests fail with "Scenarios directory not found"

The tests expect scenarios to be in `scenarios/` relative to the workspace root. Ensure you're running from the workspace root:

```bash
cd /path/to/aura
cargo test --test smoke_test
```

### All tests are skipped

This happens when no scenario files have the expected phases. Check that your scenarios have at least one `[[phases]]` section.

### TOML parsing errors

Validate your scenario TOML syntax:

```bash
# Check if file is valid TOML
python3 -c "import toml; toml.load(open('scenarios/my_scenario.toml'))"
```

## Future Enhancements

Future versions of the smoke tests will:

1. **Actually execute choreographies** using the simulator
2. **Run property checkers** to verify safety and liveness properties
3. **Simulate network conditions** (latency, packet loss, partitions)
4. **Support Byzantine scenarios** with adversarial behavior
5. **Generate execution traces** for debugging
6. **Measure performance metrics** (latency, throughput, memory)
7. **Support Quint integration** for formal verification

## Related Documentation

- `scenarios/README.md` - Scenario file format and examples
- `docs/006_simulation_engine_using_injected_effects.md` - Simulation architecture
- `crates/aura-simulator/README.md` - Simulator crate documentation
