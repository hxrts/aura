# Testing Strategies and Debugging Tools

This document describes comprehensive testing methodologies and debugging tools for Aura's distributed protocols. The testing framework combines chaos testing, property-based verification, and specialized domain testing with advanced debugging capabilities.

## Chaos Testing Framework

Chaos testing verifies protocol resilience under adverse conditions by injecting controlled faults during execution. The framework uses declarative scenario definitions to specify fault patterns and timing. Automated scenario generation creates comprehensive fault coverage without manual test case creation.

Declarative scenarios use TOML configuration files to specify fault injection patterns. Scenarios define timing windows for fault activation and deactivation. Multiple fault types can be combined within single scenarios to test complex failure modes. Scenario composition enables building complex tests from simpler fault primitives.

Byzantine fault models include message corruption, arbitrary delays, and equivocation attacks. Corrupt message injection modifies protocol messages to test input validation and error handling. Delay injection creates network partitions and timing-dependent race conditions. Equivocation injection creates conflicting messages from Byzantine participants.

Network partition scenarios test protocol behavior when participant subsets cannot communicate. Partition timing can be specified relative to protocol phases to test specific resilience mechanisms. Partition healing tests protocol recovery and state synchronization capabilities. Multiple partition patterns test complex network topology failures.

Automated scenario generation creates fault injection patterns from protocol specifications. The generator identifies critical protocol phases and creates targeted fault scenarios. Coverage analysis ensures that all protocol paths experience fault injection testing. Generated scenarios supplement manually created test cases.

Fault injection timing uses the simulation framework's time control capabilities. Faults can be triggered at specific protocol phases or after specified delays. Probabilistic fault injection creates realistic failure patterns. Coordinated fault injection across multiple participants tests distributed failure scenarios.

## Time Travel Debugging

Time travel debugging enables precise analysis of protocol execution by providing checkpoint and restore capabilities. The debugging system records complete execution state at regular intervals. Developers can navigate backward and forward through protocol execution to analyze complex distributed behavior.

Checkpoint creation captures complete system state including all participant states, message queues, and pending operations. Checkpoints occur automatically at significant protocol milestones. Manual checkpoint creation allows developers to mark specific points of interest. Checkpoint compression reduces storage requirements for long-running protocol tests.

Restore operations return the system to any previously captured checkpoint. Protocol execution can continue from restored checkpoints with modified parameters or fault injection. Multiple restore attempts enable testing different execution paths from identical starting conditions. Restore verification ensures that checkpoint data remains consistent.

Minimal reproduction automatically identifies the smallest failing scenario that reproduces a detected bug. The system performs binary search over execution steps to find the minimal trigger sequence. Environmental factors like timing and participant ordering are minimized to create focused reproduction cases. Minimal scenarios become regression tests that prevent bug reintroduction.

Execution replay enables detailed analysis of protocol behavior leading to failures. Replay preserves exact timing and message ordering from original execution. Single-step execution allows inspection of system state at each protocol operation. Breakpoint setting enables focused analysis of specific protocol phases.

State inspection provides detailed visibility into protocol state at any point during execution. Participant state dumps show internal protocol variables and data structures. Message queue inspection reveals pending and completed communication operations. Cross-participant state comparison identifies inconsistencies and synchronization issues.

Causality tracing tracks the sequence of events leading to specific outcomes. The debugging system maintains complete event histories with causal relationships. Developers can trace backward from failures to identify root causes. Forward tracing shows the consequences of specific decisions or events.

## Privacy and Security Testing

Privacy testing verifies that protocols maintain information flow boundaries and respect disclosure policies. The testing framework uses observer models to simulate different classes of adversarial monitoring. Security testing includes cryptographic protocol verification and attack simulation.

Observer models simulate external, neighbor, and group observers with different information access levels. External observers see only public network traffic without participant identity information. Neighbor observers have access to routing information and traffic patterns. Group observers have access to encrypted content through group membership.

Context isolation testing verifies that information cannot flow between protocol contexts without explicit bridging. Test scenarios attempt to correlate information across different relationship contexts. Successful isolation means that observers cannot determine whether the same participants are involved in different protocols. Failed isolation indicates potential privacy leaks.

Leakage budget enforcement testing verifies that protocols respect configured privacy limits. Test scenarios monitor information disclosure across different observer classes. Budget tracking accumulates disclosure amounts throughout protocol execution. Budget violations indicate that protocols exceed acceptable privacy boundaries.

Information flow analysis tracks data movement through protocol execution to identify potential disclosure paths. Static analysis identifies possible information flow channels in protocol code. Dynamic analysis monitors actual information disclosure during protocol execution. Flow analysis helps developers understand privacy implications of protocol design decisions.

Security testing includes verification of cryptographic protocols and simulation of various attack scenarios. Threshold signature verification ensures that ceremonies produce valid signatures with proper participant authentication. Key derivation testing verifies that derived keys maintain appropriate security properties. Attack simulation includes replay attacks, man-in-middle attacks, and collusion scenarios.

Cryptographic property testing verifies that protocols maintain required security guarantees. Forward secrecy testing ensures that past communications remain secure after key compromise. Unlinkability testing verifies that protocol executions cannot be correlated by external observers. Authentication testing ensures that participants can verify each other's identities correctly.

## Property-Based Testing Integration

Property-based testing combines with the simulation framework to generate comprehensive test coverage across large input spaces. The testing system uses random input generation with property verification to discover edge cases and boundary conditions that manual testing might miss.

Random scenario generation creates diverse test inputs including participant counts, network topologies, and timing patterns. Input constraints ensure that generated scenarios remain within realistic bounds. Shrinking algorithms automatically minimize failing scenarios to identify the essential elements that trigger failures.

Property specification defines expected protocol behavior using executable assertions. Safety properties ensure that protocols never violate correctness conditions. Liveness properties ensure that protocols eventually complete successfully. Functional properties verify that protocols produce expected outputs for given inputs.

Invariant testing verifies that protocol state remains consistent throughout execution regardless of input variation. Data structure invariants ensure that internal protocol state remains valid. Distributed invariants ensure that participant states remain synchronized appropriately. Temporal invariants ensure that protocol ordering requirements are maintained.

Coverage analysis tracks which protocol paths are exercised during property-based testing. Path coverage ensures that all code branches receive testing. State coverage ensures that all protocol states are reachable. Transition coverage ensures that all state transitions are tested.

Regression testing automatically reruns property-based tests to prevent bug reintroduction. Test case persistence saves interesting scenarios discovered during random testing. Performance regression testing detects when protocol changes impact execution efficiency. Property regression testing ensures that correctness properties remain satisfied.

## Testing Workflows

Command line tools provide comprehensive interfaces for executing testing scenarios and managing debugging sessions. The testing toolkit integrates with the existing CLI infrastructure to provide consistent developer experience. Testing workflows support both interactive debugging and automated testing execution.

Chaos testing execution uses command line tools to launch fault injection scenarios. Scenario selection allows developers to run specific fault patterns or comprehensive fault suites. Progress monitoring shows test execution status and preliminary results. Result analysis provides detailed reports on protocol resilience characteristics.

Debugging session management provides tools for creating, saving, and restoring debugging contexts. Session creation captures complete protocol state at specific execution points. Session persistence enables debugging across multiple development sessions. Session sharing enables collaborative debugging of complex distributed issues.

Property verification workflows combine formal specification checking with concrete protocol testing. Property definition uses temporal logic specifications embedded in test code. Verification execution runs protocols while monitoring for property violations. Violation analysis provides detailed feedback about failed properties.

Test suite management organizes testing scenarios into logical groups with dependency tracking. Suite execution runs multiple related tests with coordinated setup and teardown. Result aggregation provides summary reports across multiple test executions. Trend analysis tracks protocol quality metrics over time.

Performance testing workflows measure protocol execution characteristics under various conditions. Throughput testing determines maximum sustainable operation rates. Latency testing measures response times for critical protocol operations. Scalability testing evaluates protocol behavior as participant counts increase.

Developer integration provides seamless incorporation of testing tools into protocol development workflows. Test execution integrates with code changes to provide immediate feedback. Debugging tools activate automatically when test failures occur. Documentation generation creates reports suitable for protocol verification and compliance.

## Implementation Status

### ✅ Current Testing Tools (Available Today)

**Basic Testing Infrastructure**:
- `just test` - Run all tests across workspace
- `just test-crate <name>` - Test specific crate  
- `just smoke-test` - Phase 0 integration tests
- `just ci` - Full CI checks (format, lint, test)

**Property-Based Testing**:
- **Proptest integration**: [`crates/aura-testkit/`](../crates/aura-testkit/) - Shared property-based testing utilities
- **CRDT law verification**: Semilattice properties tested across all CRDT implementations
- **Crypto property tests**: [`crates/aura-crypto/tests/property_tests.rs`](../crates/aura-crypto/tests/property_tests.rs)

**Deterministic Simulation**:
- **Simulation engine**: [`crates/aura-simulator/`](../crates/aura-simulator/) - Complete deterministic testing framework
- **Injectable effects**: [`crates/aura-simulator/src/effects/system.rs`](../crates/aura-simulator/src/effects/system.rs)
- **Seeded randomness**: Deterministic PRNG for reproducible tests
- **Controlled time**: [`crates/aura-protocol/src/handlers/time/simulated.rs`](../crates/aura-protocol/src/handlers/time/simulated.rs)

**Basic Fault Injection**:
- **Chaos middleware**: [`crates/aura-simulator/src/effects/middleware/chaos_coordination.rs`](../crates/aura-simulator/src/effects/middleware/chaos_coordination.rs)
- **Fault injection**: [`crates/aura-simulator/src/effects/middleware/fault_injection.rs`](../crates/aura-simulator/src/effects/middleware/fault_injection.rs)
- **Basic Byzantine behavior**: Network partitions, message delays, controlled crashes

### ⚠️ Partial Implementation (Infrastructure Exists)

**Privacy Testing**:
- **Observer simulation**: [`crates/aura-simulator/src/privacy/`](../crates/aura-simulator/src/privacy/) - Framework exists, comprehensive tests pending
- **Privacy contracts**: [`crates/aura-mpst/tests/privacy_contracts.rs`](../crates/aura-mpst/tests/privacy_contracts.rs) - Basic property testing
- **Leakage tracking**: [`crates/aura-mpst/src/leakage.rs`](../crates/aura-mpst/src/leakage.rs) - Budget tracking implemented

**Advanced Simulation**:
- **State inspection**: [`crates/aura-simulator/src/effects/middleware/state_inspection.rs`](../crates/aura-simulator/src/effects/middleware/state_inspection.rs) - Basic capabilities
- **Time control**: [`crates/aura-simulator/src/effects/middleware/time_control.rs`](../crates/aura-simulator/src/effects/middleware/time_control.rs) - Checkpoint/restore foundation

### 🚀 Planned Features (Future Work)

**Advanced Chaos Testing**:
- Declarative TOML scenario definitions
- Automated scenario generation from protocol specs
- Byzantine fault models (equivocation, corruption)
- Complex network partition patterns

**Time Travel Debugging**:
- Complete checkpoint and restore system
- Execution replay with single-step debugging
- Minimal reproduction case generation
- Causality tracing and root cause analysis

**Enhanced Privacy Testing**:
- Statistical indistinguishability tests
- Comprehensive observer modeling
- Advanced leakage analysis and visualization
- Performance vs privacy tradeoff analysis

**Distributed Debugging**:
- Cross-participant state comparison
- Distributed state visualization
- Enhanced causality analysis
- Automated failure classification

### 📋 What You Can Test Today

**Working Test Commands**:
```bash
# Run all tests with deterministic effects
just test

# Run phase 0 integration tests
just smoke-test

# Test threshold identity functionality  
just test-dkd <app_id> <context>

# Run property-based tests for specific components
cargo test --package aura-crypto -- property_tests
cargo test --package aura-journal -- semilattice
```

**Basic Simulation Testing**:
```rust
// Create deterministic test environment
let effects = AuraEffectSystem::for_testing(device_id);

// Run protocols with controlled effects
let result = execute_protocol(&effects).await?;

// Property-based testing with proptest
proptest! {
    #[test]
    fn crdt_convergence(ops: Vec<Operation>) {
        // Test CRDT properties with random operations
    }
}
```

**What Requires Implementation**:
- Advanced chaos testing scenarios
- Time travel debugging interface
- Comprehensive privacy property verification
- Production monitoring and debugging tools

The current testing infrastructure supports Aura's 1.0 development needs with deterministic simulation, property-based testing, and basic fault injection. Advanced features will be added as the system matures.