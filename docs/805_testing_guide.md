# Testing Guide

This guide covers Aura's comprehensive testing infrastructure built on the async-first effect system architecture. Testing validates protocol correctness through property-based testing, integration testing, performance benchmarking, and provides first-class async testing support with the `#[aura_test]` macro.

## Core Testing Philosophy

Aura's testing approach is built on four key principles:

1. **Async-Native Testing** - The `#[aura_test]` macro provides automatic effect system setup and teardown with async support
2. **Deterministic Execution** - Time control, seeded randomness, and effect snapshots ensure reproducible test results
3. **Protocol Fidelity** - Tests run actual protocol logic through the real effect system, not simplified mocks
4. **WASM Compatibility** - All testing infrastructure works in both native and WebAssembly environments

This approach eliminates boilerplate while providing powerful testing capabilities through automatic context propagation, scoped test isolation, and comprehensive test utilities.

## Property-Based Testing

Property-based testing validates protocol correctness across diverse input spaces using the stateless effect system. Properties express invariants that must hold for all valid inputs and execution scenarios.

### Protocol Properties with Async Testing

Define fundamental properties using the `#[aura_test]` macro:

```rust
use proptest::prelude::*;
use aura_testkit::{aura_test, TestContext, freeze_time, advance_time_by};

pub struct ConsistencyProperty;

impl ProtocolProperty for ConsistencyProperty {
    async fn check_property(
        &self,
        ctx: &TestContext,
        execution_trace: &ExecutionTrace,
    ) -> PropertyResult {
        // Time is automatically controlled in tests
        freeze_time();
        
        // Execute protocol with deterministic timing
        let final_states = ctx.execute_protocol().await?;
        
        // Advance time for next phase
        advance_time_by(Duration::from_secs(5));
        
        // Check state convergence
        let reference_state = &final_states[0];
        for state in &final_states[1..] {
            if !states_are_equivalent(reference_state, state) {
                return PropertyResult::Violated {
                    description: "State convergence failed".to_string(),
                    counterexample: format!("States diverged after {} seconds", 
                                          ctx.elapsed().as_secs()),
                };
            }
        }
        
        // Verify through effect snapshots
        let snapshot = ctx.effects().snapshot();
        assert_eq!(snapshot.total_operations(), execution_trace.len());
        
        PropertyResult::Satisfied
    }
}

#[aura_test]
async fn test_consistency_property() {
    let property = ConsistencyProperty;
    let result = property.check_property(&ctx, &trace).await;
    assert_eq!(result, PropertyResult::Satisfied);
}
```

Consistency properties verify eventual convergence across all devices. Causal consistency ensures operations respect dependency relationships. Property violations provide counterexamples for debugging.

### Safety and Liveness Properties

Test critical safety and liveness guarantees:

```rust
use aura_testkit::{ProtocolTestFixture, TestEffectsBuilder, TestExecutionMode};

pub struct SafetyProperty {
    invariant: Box<dyn Fn(&SystemState) -> bool + Send + Sync>,
    description: String,
}

impl SafetyProperty {
    pub fn never_double_spend() -> Self {
        Self {
            invariant: Box::new(|state| {
                let total_issued = state.total_tokens_issued();
                let total_held = state.total_tokens_held_by_devices();
                total_issued >= total_held
            }),
            description: "Total tokens held never exceeds total tokens issued".to_string(),
        }
    }
}

impl ProtocolProperty for SafetyProperty {
    async fn check_property(
        &self,
        fixtures: &[ProtocolTestFixture],
        execution_trace: &ExecutionTrace,
    ) -> PropertyResult {
        for checkpoint in &execution_trace.state_checkpoints {
            if !(self.invariant)(&checkpoint.system_state) {
                return PropertyResult::Violated {
                    description: self.description.clone(),
                    counterexample: format!("Violation at timestamp {}: {:?}",
                                          checkpoint.timestamp, checkpoint.system_state),
                };
            }
        }

        PropertyResult::Satisfied
    }
}

pub struct LivenessProperty {
    progress_condition: Box<dyn Fn(&ExecutionTrace) -> bool + Send + Sync>,
    timeout: std::time::Duration,
}

impl LivenessProperty {
    pub fn eventual_consensus(consensus_timeout: std::time::Duration) -> Self {
        Self {
            progress_condition: Box::new(|trace| {
                trace.events.iter().any(|event| match event {
                    ExecutionEvent::ConsensusReached { .. } => true,
                    _ => false,
                })
            }),
            timeout: consensus_timeout,
        }
    }
}
```

Safety properties verify that bad things never happen during execution. Liveness properties ensure that good things eventually happen within specified timeouts. Both property types are essential for protocol correctness.

### Randomized Input Generation with Network Simulation

Generate diverse test inputs with network simulation support:

```rust
use proptest::strategy::Strategy;
use aura_testkit::{NetworkSimulator, aura_test};

pub fn arbitrary_protocol_scenario() -> impl Strategy<Value = ProtocolScenario> {
    (
        1..=10_usize, // device count
        proptest::collection::vec(arbitrary_message(), 1..=100), // message sequence
        arbitrary_network_conditions(),
        proptest::option::of(arbitrary_failure_pattern()),
    ).prop_map(|(device_count, messages, network_conditions, failures)| {
        ProtocolScenario {
            device_count,
            message_sequence: messages,
            network_conditions,
            failure_pattern: failures,
        }
    })
}

#[aura_test(timeout = "60s")]
async fn test_protocol_under_network_conditions(scenario: ProtocolScenario) {
    // Network simulator automatically available
    let mut sim = NetworkSimulator::new();
    
    // Configure network conditions
    sim.add_latency(scenario.network_conditions.latency);
    sim.add_jitter(scenario.network_conditions.jitter);
    sim.add_packet_loss(scenario.network_conditions.loss_rate);
    
    // Add network partitions if specified
    if let Some(partition) = scenario.network_conditions.partition {
        sim.add_partition(partition.group_a, partition.group_b);
    }
    
    // Execute protocol with simulated network
    let result = execute_protocol_with_simulator(&scenario, &sim).await?;
    
    // Verify convergence despite network conditions
    assert!(result.converged_within(Duration::from_secs(30)));
}

pub fn arbitrary_message() -> impl Strategy<Value = ProtocolMessage> {
    prop_oneof![
        Just(ProtocolMessage::Ping),
        any::<String>().prop_map(ProtocolMessage::Data),
        (any::<u64>(), any::<u32>()).prop_map(|(timestamp, cost)| ProtocolMessage::FlowUpdate { timestamp, cost }),
    ]
}

proptest! {
    #[aura_test]
    async fn protocol_maintains_consistency(scenario in arbitrary_protocol_scenario()) {
        // Test context automatically available
        let consistency_property = ConsistencyProperty;
        
        // Execute scenario with automatic setup
        let trace = execute_protocol_scenario(&ctx, &scenario).await?;
        
        // Verify property
        let result = consistency_property.check_property(&ctx, &trace).await?;
        assert_eq!(result, PropertyResult::Satisfied);
        
        // Effect snapshots for additional validation
        let snapshot = ctx.effects().snapshot();
        assert!(snapshot.all_operations_completed());
        assert_eq!(snapshot.failed_operations(), 0);
    }
}
```

Randomized testing explores edge cases that manual testing might miss. Property-based tests generate thousands of scenarios automatically. Strategic input generation focuses on protocol-relevant scenarios.

## Integration Testing

Integration testing validates complete system behavior across multiple protocol layers. These tests verify correct interaction between authentication, authorization, storage, and network layers.

### End-to-End Protocol Testing with Test Fixtures

Test complete protocol workflows using test fixtures:

```rust
use aura_testkit::{aura_test, TestFixture, TestContext};

#[aura_test]
async fn test_threshold_signing_workflow() {
    // Create test fixture with pre-configured setup
    let fixture = TestFixture::new()
        .with_device_count(5)
        .with_threshold(3)
        .with_mocks()  // Use mock handlers for speed
        .build();
    
    let message = b"integration test message";
    let devices = fixture.devices();
    
    // Phase 1: Initialize signing session
    let session_id = fixture.initiate_threshold_signing(
        &devices,
        message,
    ).await?;
    
    // Phase 2: Collect partial signatures (with time control)
    freeze_time();
    let threshold_devices = &devices[..3];
    let partial_signatures = fixture.collect_partial_signatures(
        session_id,
        threshold_devices,
    ).await?;
    advance_time_by(Duration::from_secs(1));
    
    // Phase 3: Aggregate signature
    let final_signature = fixture.aggregate_signatures(
        session_id,
        partial_signatures,
    ).await?;
    
    // Phase 4: Verify with effect snapshots
    let snapshot = ctx.effects().snapshot();
    assert_eq!(snapshot.crypto_operations(), 4); // 3 partial + 1 aggregate
    assert_eq!(snapshot.network_messages(), 6);  // Protocol messages
    
    // Verify signature
    let verification_result = fixture.verify_threshold_signature(
        message,
        &final_signature,
        &devices,
    ).await?;
    
    assert!(verification_result.is_valid());
    assert_eq!(verification_result.signing_devices().len(), 3);
}

    pub async fn test_complete_signing_workflow(&self) -> Result<(), TestError> {
        let message = b"integration test message";
        let devices: Vec<_> = self.fixtures.iter().map(|f| f.device_id()).collect();

        // Phase 1: Initialize signing session using choreography
        let session_id = self.harness.initiate_threshold_signing(
            &devices,
            message,
        ).await?;

        // Phase 2: Collect partial signatures from threshold devices
        let threshold_devices = &devices[..2]; // Use threshold number of devices
        let partial_signatures = self.harness.collect_partial_signatures(
            session_id,
            threshold_devices,
        ).await?;

        // Phase 3: Aggregate signature
        let final_signature = self.harness.aggregate_signatures(
            session_id,
            partial_signatures,
        ).await?;

        // Phase 4: Verify aggregated signature
        let verification_result = self.harness.verify_threshold_signature(
            message,
            &final_signature,
            &devices,
        ).await?;

        assert!(verification_result.is_valid());
        assert_eq!(verification_result.signing_devices().len(), 2);

        Ok(())
    }
}
```

Integration tests verify complete protocol execution across all system layers. Phase-based testing validates each step of complex multi-party protocols. End-to-end verification ensures correctness of the entire workflow.

### Cross-Layer Validation with Context Propagation

Test interactions between system layers with automatic context:

```rust
use aura_testkit::{aura_test, current_context, with_context};

#[aura_test]
async fn test_capability_journal_integration() {
    // Context automatically available
    let initial_capabilities = create_test_capability_set();
    
    // Initialize journal with capabilities
    let journal_entry = ctx.effects().journal()
        .create_entry_with_context(
            &current_context(),
            initial_capabilities.clone(),
        )
        .await?;
    
    // Perform capability-gated operation
    let operation = TreeOperation::AddMember {
        member_id: aura_core::DeviceId::new(),
        capabilities: create_restricted_capability_set(),
    };
    
    // Context flows through validation
    let capability_check = ctx.effects()
        .validate_capability_for_operation(
            &operation,
            &initial_capabilities,
        )
        .await?;
    
    assert!(capability_check.is_authorized());
    
    // Execute with nested context for tracing
    let result = with_context(ctx.child("journal_operation"), async {
        ctx.effects().journal()
            .execute_operation(
                &journal_entry,
                operation.clone(),
                capability_check.authorization_proof(),
            )
            .await
    }).await?;
    
    // Verify through snapshots
    let snapshot = ctx.effects().snapshot();
    assert_eq!(snapshot.journal_operations(), 1);
    assert!(snapshot.operation_succeeded(&operation));
    
    // Context automatically includes flow budget tracking
    assert!(ctx.flow_budget_spent() > 0);
}
```

Cross-layer validation tests ensure clean interfaces between system components. Capability and journal integration validates authorization and state management. Layer interactions must maintain security and consistency properties.

### Recovery and Resilience Testing with Fault Injection

Test system recovery using built-in fault injection:

```rust
use aura_testkit::{aura_test, FaultInjector, NetworkSimulator};

#[aura_test(no_deterministic_time)]
async fn test_byzantine_fault_tolerance() {
    let device_count = 5;
    let byzantine_count = 1; // f < n/3
    
    // Create devices with one Byzantine
    let mut devices = Vec::new();
    for i in 0..device_count {
        if i < byzantine_count {
            // Byzantine device with fault injection
            let fault_injector = FaultInjector::new()
                .corrupt_messages(0.5)  // 50% message corruption
                .delay_messages(100..500)  // Random delays
                .drop_messages(0.1);   // 10% message drop
            
            devices.push(ctx.create_byzantine_device(fault_injector).await?);
        } else {
            devices.push(ctx.create_honest_device().await?);
        }
    }
    
    // Execute protocol under Byzantine conditions
    let result = execute_protocol(&devices).await;
    
    // Protocol should succeed despite Byzantine behavior
    assert!(result.is_ok());
    
    // Verify all honest devices converged
    let honest_states: Vec<_> = devices[byzantine_count..]
        .iter()
        .map(|d| d.final_state())
        .collect();
    
    let reference = &honest_states[0];
    for state in &honest_states[1..] {
        assert_eq!(state.merkle_root(), reference.merkle_root());
    }
    
    // Check Byzantine device was detected
    let snapshot = ctx.effects().snapshot();
    assert!(snapshot.byzantine_detections() >= 1);
}

#[aura_test]
async fn test_network_partition_recovery() {
    let devices = ctx.create_devices(5).await?;
    
    // Configure network with partition
    let mut sim = NetworkSimulator::new();
    let partition_a = devices[..3].to_vec();
    let partition_b = devices[3..].to_vec();
    
    sim.add_partition(partition_a.clone(), partition_b.clone());
    
    // Start protocol
    let protocol_handle = spawn_protocol(&devices);
    
    // Let it run for a while with partition
    advance_time_by(Duration::from_secs(10));
    
    // Heal partition
    sim.remove_partition();
    
    // Protocol should complete
    let result = protocol_handle.await?;
    assert!(result.completed_successfully());
    
    // All devices should converge
    verify_convergence(&devices).await?;
}

    pub async fn test_byzantine_fault_tolerance(&self) -> Result<ResilienceReport, TestError> {
        let byzantine_device_count = self.baseline_fixtures.len() / 3; // f < n/3
        let mut reports = Vec::new();

        for scenario in &self.recovery_scenarios {
            let mut test_fixtures = self.baseline_fixtures.clone();

            // Create Byzantine effect systems for selected devices
            let byzantine_devices = select_random_devices(&test_fixtures, byzantine_device_count);
            for &device_idx in &byzantine_devices {
                let byzantine_builder = TestEffectsBuilder::for_simulation(
                    test_fixtures[device_idx].device_id()
                )
                .with_seed(test_fixtures[device_idx].seed())
                .with_byzantine_behavior(scenario.byzantine_behavior.clone());

                // Replace fixture with Byzantine version
                test_fixtures[device_idx] = ProtocolTestFixture::from_effects_builder(
                    byzantine_builder,
                    self.config.threshold,
                    self.config.total_devices,
                ).await?;
            }

            // Execute protocol under Byzantine conditions
            let execution_result = execute_protocol_scenario(&test_fixtures, scenario).await;

            let scenario_report = RecoveryScenarioReport {
                scenario: scenario.clone(),
                byzantine_devices: byzantine_devices.clone(),
                execution_result,
                protocol_succeeded: execution_result.is_ok(),
                consistency_maintained: check_consistency_across_fixtures(&test_fixtures).await?,
            };

            reports.push(scenario_report);
        }

        Ok(ResilienceReport {
            total_scenarios: reports.len(),
            successful_recoveries: reports.iter().filter(|r| r.protocol_succeeded).count(),
            consistency_violations: reports.iter().filter(|r| !r.consistency_maintained).count(),
            scenario_reports: reports,
        })
    }
}
```

Recovery testing validates system behavior under adversarial conditions. Byzantine fault tolerance testing ensures security against malicious participants. Resilience reports provide quantitative assessment of system robustness.

## Performance Benchmarking

Performance benchmarking measures protocol efficiency and scalability characteristics. Benchmarks validate performance requirements and identify optimization opportunities.

### Protocol Performance Metrics with Async Benchmarking

Measure performance using async-aware benchmarking:

```rust
use aura_testkit::{aura_test, PerformanceMonitor};

#[aura_test(capture)]
async fn benchmark_threshold_signing() {
    let monitor = PerformanceMonitor::new();
    
    // Benchmark different device counts
    for device_count in [3, 5, 10, 20] {
        let devices = ctx.create_devices(device_count).await?;
        
        // Measure initialization
        let init_time = monitor.measure("initialization", async {
            initialize_protocol(&devices).await
        }).await?;
        
        // Measure signing rounds
        let signing_time = monitor.measure("signing", async {
            execute_threshold_signing(&devices[..3], b"test message").await
        }).await?;
        
        // Measure verification
        let verify_time = monitor.measure("verification", async {
            verify_threshold_signature(&signature, &devices).await
        }).await?;
        
        println!("Device count: {}", device_count);
        println!("  Init: {:?}", init_time);
        println!("  Sign: {:?}", signing_time);
        println!("  Verify: {:?}", verify_time);
    }
    
    // Generate performance report
    let report = monitor.generate_report();
    assert!(report.p99_latency < Duration::from_secs(1));
    assert!(report.throughput > 100); // ops/sec
}

    pub async fn benchmark_with_parallel_init(
        &self,
        device_counts: Vec<usize>,
        message_sizes: Vec<usize>,
        iterations: usize,
    ) -> Result<BenchmarkReport, BenchmarkError> {
        let mut results = Vec::new();

        for &device_count in &device_counts {
            for &message_size in &message_sizes {
                let mut iteration_results = Vec::new();

                for iteration in 0..iterations {
                    let test_message = vec![0u8; message_size];
                    
                    // Use parallel initialization for performance
                    let builder = ParallelInitBuilder::new(self.config.clone())
                        .with_device_count(device_count)
                        .with_metrics();
                    
                    let (system, init_metrics) = builder.build().await?;
                    let setup_duration = init_metrics.unwrap().total_duration;

                    // Measure signing latency
                    let signing_start = Instant::now();
                    let devices: Vec<_> = fixtures.iter().map(|f| f.device_id()).collect();
                    let session_id = harness.setup_threshold_session(&devices).await?;
                    let signature = harness.execute_threshold_signing(
                        session_id,
                        &test_message,
                        &devices[..self.config.threshold as usize], // Use threshold devices
                    ).await?;
                    let signing_duration = signing_start.elapsed();

                    // Measure verification latency
                    let verification_start = Instant::now();
                    let verification_result = harness.verify_threshold_signature(
                        &test_message,
                        &signature,
                        &devices,
                    ).await?;
                    let verification_duration = verification_start.elapsed();

                    let total_duration = start_time.elapsed();

                    iteration_results.push(IterationMetrics {
                        setup_duration,
                        signing_duration,
                        verification_duration,
                        total_duration,
                        message_size,
                        device_count,
                        success: verification_result.is_valid(),
                    });
                }

                let scenario_metrics = ScenarioMetrics::aggregate(iteration_results);
                results.push(scenario_metrics);
            }
        }

        Ok(BenchmarkReport {
            protocol: "threshold_signing".to_string(),
            scenario_results: results,
            summary: BenchmarkSummary::from_results(&results),
        })
    }
}
```

Performance benchmarking measures latency across different protocol phases. Parameterized testing evaluates scalability with varying device counts and message sizes. Statistical aggregation provides reliable performance characterization.

### Memory and Resource Profiling with Allocation Tracking

Profile resource usage with built-in allocation tracking:

```rust
use aura_testkit::{aura_test, AllocationTracker, MemoryProfiler};

#[aura_test]
async fn profile_journal_memory_usage() {
    let profiler = MemoryProfiler::new();
    let tracker = AllocationTracker::new();
    
    // Test with different operation counts
    for operation_count in [100, 1000, 10000] {
        // Take baseline snapshot
        let baseline = profiler.snapshot();
        
        // Track allocations during operations
        let _guard = tracker.track_allocations();
        
        // Execute journal operations
        let journal = ctx.effects().journal();
        for i in 0..operation_count {
            let operation = create_test_operation(i);
            journal.apply_operation(&operation).await?;
            
            // Periodic snapshots
            if i % 1000 == 0 {
                let snapshot = profiler.snapshot();
                let allocations = tracker.allocations_since(&baseline);
                
                println!("After {} operations:", i);
                println!("  Heap: {} MB", snapshot.heap_mb());
                println!("  Allocations: {}", allocations.count);
                println!("  Allocation rate: {}/op", 
                         allocations.count / (i + 1));
            }
        }
        
        // Final analysis
        let final_snapshot = profiler.snapshot();
        let total_allocations = tracker.allocations_since(&baseline);
        
        // Verify memory efficiency
        let bytes_per_op = (final_snapshot.heap_bytes() - baseline.heap_bytes()) 
                          / operation_count;
        assert!(bytes_per_op < 1024); // Less than 1KB per operation
        
        // Check for memory leaks
        assert!(total_allocations.leaked_bytes == 0);
    }
}

            let population_start = Instant::now();
            let mut journal_state = fixture.account_state().clone();

            for i in 0..operation_count {
                let operation = create_test_operation(i, device_id);

                // Apply operation through stateless effects
                let operation_result = effects.apply_journal_operation(
                    &journal_state,
                    operation,
                ).await?;

                journal_state = operation_result.updated_state;

                // Take periodic snapshots during population
                if i % 1000 == 0 {
                    let snapshot = self.profiler.take_memory_snapshot().await?;
                    profiles.push(MemoryDataPoint {
                        operation_count: i,
                        heap_usage: snapshot.heap_size,
                        stack_usage: snapshot.stack_size,
                        journal_size: journal_state.serialized_size(),
                        timestamp: population_start.elapsed(),
                    });
                }
            }

            let final_snapshot = self.profiler.take_memory_snapshot().await?;

            profiles.push(MemoryDataPoint {
                operation_count,
                heap_usage: final_snapshot.heap_size,
                stack_usage: final_snapshot.stack_size,
                journal_size: journal_state.serialized_size(),
                timestamp: population_start.elapsed(),
            });
        }

        Ok(MemoryProfileReport {
            component: "journal".to_string(),
            memory_progression: profiles,
            peak_memory: profiles.iter().map(|p| p.heap_usage).max().unwrap_or(0),
            memory_efficiency: calculate_memory_efficiency(&profiles),
        })
    }
}
```

Memory profiling tracks resource usage throughout protocol execution. Memory progression analysis identifies memory leaks and optimization opportunities. Efficiency metrics compare actual usage to theoretical minimums.

## Testing Best Practices

Comprehensive testing combines the `#[aura_test]` macro with specialized test utilities for thorough validation. The macro eliminates boilerplate while providing powerful features through attributes.

### Using the #[aura_test] Macro

```rust
use aura_testkit::{aura_test, TestFixture, NetworkSimulator};

// Basic async test with automatic setup
#[aura_test]
async fn test_basic_protocol() {
    // Effect system automatically initialized
    let result = execute_protocol().await?;
    assert!(result.success);
}

// Test with custom timeout
#[aura_test(timeout = "30s")]
async fn test_long_running_protocol() {
    // Test will fail if not completed within 30 seconds
}

// Test without automatic initialization
#[aura_test(no_init)]
async fn test_manual_setup() {
    // Manual effect system setup required
    let system = create_custom_effect_system().await?;
}

// Test with output capture for debugging
#[aura_test(capture)]
async fn test_with_debugging() {
    println!("This output will be captured");
    // Output only shown if test fails
}

// Test without deterministic time
#[aura_test(no_deterministic_time)]
async fn test_real_time_behavior() {
    // Uses actual system time instead of controlled time
}
```

impl ComprehensiveTestSuite {
    pub fn new(protocol_name: &str, config: StatelessFixtureConfig) -> Self {
        Self {
            protocol_name: protocol_name.to_string(),
            config,
            unit_tests: Vec::new(),
            integration_tests: Vec::new(),
            property_tests: Vec::new(),
            benchmarks: Vec::new(),
        }
    }

### Comprehensive Test Suites

```rust
pub struct ComprehensiveTestSuite {
    protocol_name: String,
    test_fixture: TestFixture,
}

impl ComprehensiveTestSuite {
    pub fn new(protocol_name: &str) -> Self {
        Self {
            protocol_name: protocol_name.to_string(),
            test_fixture: TestFixture::new()
                .with_mocks()
                .with_deterministic_time()
                .build(),
        }
    }
    
    pub async fn run_validation(&mut self) -> Result<ValidationReport> {
        let mut report = ValidationReport::new(&self.protocol_name);
        
        // Run unit tests
        report.add_section("Unit Tests", self.run_unit_tests().await?);
        
        // Run integration tests with network simulation
        let mut sim = NetworkSimulator::new();
        sim.add_latency(10..50);
        report.add_section("Integration Tests", 
            self.run_integration_tests(&sim).await?);
        
        // Run property tests
        report.add_section("Property Tests", 
            self.run_property_tests().await?);
        
        // Run performance benchmarks
        let monitor = PerformanceMonitor::new();
        report.add_section("Performance", 
            self.run_benchmarks(&monitor).await?);
        
        Ok(report)
    }
}

### Test Utilities and Helpers

```rust
// Time control utilities
#[aura_test]
async fn test_with_time_control() {
    // Freeze time at current moment
    freeze_time();
    
    let start = current_time();
    execute_operation().await?;
    
    // Time hasn't advanced
    assert_eq!(current_time(), start);
    
    // Advance time explicitly
    advance_time_by(Duration::from_secs(10));
    assert_eq!(current_time(), start + Duration::from_secs(10));
}

// Network simulation utilities
#[aura_test]
async fn test_with_network_conditions() {
    let mut sim = NetworkSimulator::new();
    
    // Add various network conditions
    sim.add_latency(50..150);        // 50-150ms latency
    sim.add_jitter(10);              // ±10ms jitter
    sim.add_packet_loss(0.02);       // 2% packet loss
    sim.add_bandwidth_limit(1_000_000); // 1MB/s
    
    // Create partition between device groups
    let group_a = vec![device1, device2];
    let group_b = vec![device3, device4];
    sim.add_partition(group_a, group_b);
    
    // Execute protocol under these conditions
    let result = execute_with_simulator(&sim).await?;
    assert!(result.completed_despite_conditions());
}

// Effect snapshot utilities
#[aura_test]
async fn test_with_effect_snapshots() {
    // Take snapshot before operation
    let before = ctx.effects().snapshot();
    
    // Execute operations
    perform_protocol_operations().await?;
    
    // Take snapshot after
    let after = ctx.effects().snapshot();
    
    // Analyze differences
    let diff = after.diff(&before);
    assert_eq!(diff.network_calls, 5);
    assert_eq!(diff.crypto_operations, 3);
    assert_eq!(diff.storage_writes, 2);
    
    // Verify specific operations
    assert!(after.contains_operation("threshold_sign"));
    assert_eq!(after.operation_count("send_message"), 5);
}
```
```

Test suite composition provides systematic validation coverage. Automated test discovery reduces maintenance overhead. The stateless architecture enables clean separation between different test types.

## Summary

Aura's async-native testing infrastructure provides:

- **Zero Boilerplate** - The `#[aura_test]` macro handles all setup and teardown
- **Time Control** - Deterministic time for reproducible tests
- **Network Simulation** - Comprehensive network condition modeling
- **Effect Snapshots** - Detailed visibility into system behavior
- **WASM Compatibility** - All utilities work in browser environments
- **Performance Monitoring** - Built-in benchmarking and profiling

The testing approach eliminates traditional trade-offs between test fidelity and ease of use, enabling comprehensive validation of distributed protocols with minimal effort.

For simulation capabilities that enable comprehensive fault injection and deterministic execution, see [Simulation Guide](806_simulation_guide.md). Learn effect system details in [System Architecture](002_system_architecture.md). Review the async refactor progress in [Async Test Plan](../work/async_test.md).
