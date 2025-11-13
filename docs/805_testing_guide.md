# Testing Guide

This guide covers Aura's comprehensive testing infrastructure built on the stateless effect system architecture. Testing validates protocol correctness through property-based testing, integration testing, and performance benchmarking.

## Core Testing Philosophy

Aura's testing approach is built on three key principles. Tests run actual protocol logic rather than simplified mocks. All randomness and timing uses deterministic seeds for reproducible results. Testing integrates with the stateless effect system for clean isolation.

This approach provides superior validation compared to traditional integration testing. The testkit infrastructure enables comprehensive coverage without complex mocking or brittle test fixtures.

## Property-Based Testing

Property-based testing validates protocol correctness across diverse input spaces using the stateless effect system. Properties express invariants that must hold for all valid inputs and execution scenarios.

### Protocol Properties

Define fundamental properties for distributed protocols using stateless fixtures:

```rust
use proptest::prelude::*;
use aura_testkit::{ProtocolTestFixture, TestEffectsBuilder, TestExecutionMode};

pub struct ConsistencyProperty;

impl ProtocolProperty for ConsistencyProperty {
    async fn check_property(
        &self,
        fixtures: &[ProtocolTestFixture],
        execution_trace: &ExecutionTrace,
    ) -> PropertyResult {
        // Collect final states from stateless fixtures
        let final_states: Vec<_> = fixtures
            .iter()
            .map(|fixture| fixture.account_state())
            .collect();

        // Check state convergence across all devices
        let reference_state = &final_states[0];
        for state in &final_states[1..] {
            if !states_are_equivalent(reference_state, state) {
                return PropertyResult::Violated {
                    description: "State convergence failed".to_string(),
                    counterexample: format!("Reference: {:?}, Divergent: {:?}", reference_state, state),
                };
            }
        }

        // Check causal consistency using deterministic seeds
        for (i, fixture) in fixtures.iter().enumerate() {
            let device_operations = extract_operations_for_device(execution_trace, fixture.device_id());
            if !validate_causal_ordering_stateless(&device_operations, fixture.seed()) {
                return PropertyResult::Violated {
                    description: "Causal consistency violated".to_string(),
                    counterexample: format!("Device {}: {:?}", i, device_operations),
                };
            }
        }

        PropertyResult::Satisfied
    }
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

### Randomized Input Generation

Generate diverse test inputs for comprehensive coverage:

```rust
use proptest::strategy::Strategy;
use aura_testkit::{ProtocolTestFixture, StatelessFixtureConfig, TestExecutionMode};

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

pub fn arbitrary_message() -> impl Strategy<Value = ProtocolMessage> {
    prop_oneof![
        Just(ProtocolMessage::Ping),
        any::<String>().prop_map(ProtocolMessage::Data),
        (any::<u64>(), any::<u32>()).prop_map(|(timestamp, cost)| ProtocolMessage::FlowUpdate { timestamp, cost }),
    ]
}

proptest! {
    #[test]
    fn protocol_maintains_consistency(scenario in arbitrary_protocol_scenario()) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            // Create fixtures using stateless architecture
            let mut fixtures = Vec::new();
            for i in 0..scenario.device_count {
                let config = StatelessFixtureConfig {
                    execution_mode: TestExecutionMode::Simulation,
                    seed: 42 + i as u64,
                    threshold: 2,
                    total_devices: scenario.device_count as u16,
                    primary_device: None,
                };
                let fixture = ProtocolTestFixture::with_stateless_effects(
                    config.threshold,
                    config.total_devices,
                    config.execution_mode,
                    config.seed,
                ).await?;
                fixtures.push(fixture);
            }

            let trace = execute_protocol_scenario(&fixtures, &scenario).await?;
            let consistency_property = ConsistencyProperty;

            assert_eq!(
                consistency_property.check_property(&fixtures, &trace).await?,
                PropertyResult::Satisfied
            );
        });
    }
}
```

Randomized testing explores edge cases that manual testing might miss. Property-based tests generate thousands of scenarios automatically. Strategic input generation focuses on protocol-relevant scenarios.

## Integration Testing

Integration testing validates complete system behavior across multiple protocol layers. These tests verify correct interaction between authentication, authorization, storage, and network layers.

### End-to-End Protocol Testing

Test complete protocol workflows from initialization to completion:

```rust
use aura_testkit::{ChoreographyTestHarness, ProtocolTestFixture, TestExecutionMode, StatelessFixtureConfig};

pub struct ThresholdSigningIntegrationTest {
    fixtures: Vec<ProtocolTestFixture>,
    harness: ChoreographyTestHarness,
}

impl ThresholdSigningIntegrationTest {
    pub async fn setup(device_count: usize, threshold: usize) -> Result<Self, TestError> {
        // Create fixtures using stateless architecture
        let mut fixtures = Vec::new();
        for i in 0..device_count {
            let config = StatelessFixtureConfig {
                execution_mode: TestExecutionMode::Integration,
                seed: 100 + i as u64,
                threshold: threshold as u16,
                total_devices: device_count as u16,
                primary_device: None,
            };
            let fixture = ProtocolTestFixture::with_stateless_effects(
                config.threshold,
                config.total_devices,
                config.execution_mode,
                config.seed,
            ).await?;
            fixtures.push(fixture);
        }

        let harness = ChoreographyTestHarness::from_fixtures(
            fixtures.clone(),
            TestExecutionMode::Integration,
        ).await?;

        Ok(Self { fixtures, harness })
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

### Cross-Layer Validation

Test interactions between different system layers:

```rust
use aura_testkit::{ProtocolTestFixture, TestEffectsBuilder, TestExecutionMode};

pub async fn test_capability_journal_integration() -> Result<(), IntegrationError> {
    let device_id = aura_core::DeviceId::new();

    // Create test fixture with stateless effects
    let fixture = ProtocolTestFixture::for_integration_tests(device_id).await?;
    let initial_capabilities = create_test_capability_set();

    // Build effect system for integration testing
    let effects_builder = TestEffectsBuilder::for_integration_tests(device_id)
        .with_seed(fixture.seed());
    let effects = effects_builder.build()?;

    // Initialize journal with capabilities using stateless approach
    let journal_entry = effects.create_journal_entry(
        device_id,
        initial_capabilities.clone(),
    ).await?;

    // Perform capability-gated operation
    let operation = TreeOperation::AddMember {
        member_id: aura_core::DeviceId::new(),
        capabilities: create_restricted_capability_set(),
    };

    // Verify capability checking through stateless effects
    let capability_check = effects.validate_capability_for_operation(
        &operation,
        &initial_capabilities,
    ).await?;

    assert!(capability_check.is_authorized());

    // Execute operation through stateless journal effects
    let operation_result = effects.execute_journal_operation(
        &journal_entry,
        operation.clone(),
        capability_check.authorization_proof(),
    ).await?;

    // Verify operation success and journal consistency
    assert!(operation_result.is_successful());
    assert!(operation_result.journal_updated());
    assert!(operation_result.contains_operation_evidence(&operation));

    Ok(())
}
```

Cross-layer validation tests ensure clean interfaces between system components. Capability and journal integration validates authorization and state management. Layer interactions must maintain security and consistency properties.

### Recovery and Resilience Testing

Test system recovery under various failure conditions:

```rust
use aura_testkit::{ProtocolTestFixture, TestEffectsBuilder, TestExecutionMode, StatelessFixtureConfig};

pub struct RecoveryTestSuite {
    baseline_fixtures: Vec<ProtocolTestFixture>,
    recovery_scenarios: Vec<RecoveryScenario>,
    config: StatelessFixtureConfig,
}

impl RecoveryTestSuite {
    pub async fn new(
        device_count: usize,
        config: StatelessFixtureConfig,
    ) -> Result<Self, TestError> {
        let mut fixtures = Vec::new();
        for i in 0..device_count {
            let device_config = StatelessFixtureConfig {
                seed: config.seed + i as u64,
                ..config.clone()
            };
            let fixture = ProtocolTestFixture::with_stateless_effects(
                device_config.threshold,
                device_config.total_devices,
                device_config.execution_mode,
                device_config.seed,
            ).await?;
            fixtures.push(fixture);
        }

        Ok(Self {
            baseline_fixtures: fixtures,
            recovery_scenarios: create_standard_recovery_scenarios(),
            config,
        })
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

### Protocol Performance Metrics

Measure key performance indicators for distributed protocols:

```rust
use std::time::{Instant, Duration};
use aura_testkit::{ChoreographyTestHarness, ProtocolTestFixture, TestExecutionMode, StatelessFixtureConfig};

pub struct ProtocolBenchmark {
    config: StatelessFixtureConfig,
    metrics_collector: MetricsCollector,
}

impl ProtocolBenchmark {
    pub fn new(config: StatelessFixtureConfig) -> Self {
        Self {
            config,
            metrics_collector: MetricsCollector::new(),
        }
    }

    pub async fn benchmark_threshold_signing(
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

                    // Create stateless test fixtures for this iteration
                    let mut fixtures = Vec::new();
                    for i in 0..device_count {
                        let iteration_config = StatelessFixtureConfig {
                            seed: self.config.seed + (iteration * device_count + i) as u64,
                            execution_mode: TestExecutionMode::Simulation,
                            ..self.config.clone()
                        };
                        let fixture = ProtocolTestFixture::with_stateless_effects(
                            iteration_config.threshold,
                            device_count as u16,
                            iteration_config.execution_mode,
                            iteration_config.seed,
                        ).await?;
                        fixtures.push(fixture);
                    }

                    let start_time = Instant::now();

                    // Measure setup overhead
                    let setup_start = Instant::now();
                    let harness = ChoreographyTestHarness::from_fixtures(
                        fixtures.clone(),
                        TestExecutionMode::Simulation,
                    ).await?;
                    let setup_duration = setup_start.elapsed();

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

### Memory and Resource Profiling

Profile resource usage during protocol execution:

```rust
use std::time::Instant;
use aura_testkit::{ProtocolTestFixture, TestEffectsBuilder, TestExecutionMode, StatelessFixtureConfig};

pub struct ResourceBenchmark {
    profiler: ResourceProfiler,
    config: StatelessFixtureConfig,
}

impl ResourceBenchmark {
    pub fn new(config: StatelessFixtureConfig) -> Self {
        Self {
            profiler: ResourceProfiler::new(),
            config,
        }
    }

    pub async fn profile_journal_memory_usage(
        &self,
        operation_counts: Vec<usize>,
    ) -> Result<MemoryProfileReport, ProfilingError> {
        let mut profiles = Vec::new();
        let device_id = aura_core::DeviceId::new();

        for operation_count in operation_counts {
            let baseline_snapshot = self.profiler.take_memory_snapshot().await?;

            // Create test fixture with stateless effects
            let fixture = ProtocolTestFixture::with_stateless_effects(
                self.config.threshold,
                self.config.total_devices,
                TestExecutionMode::Simulation,
                self.config.seed,
            ).await?;

            // Build stateless effect system for journal operations
            let effects_builder = TestEffectsBuilder::for_simulation(device_id)
                .with_seed(fixture.seed());
            let effects = effects_builder.build()?;

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

Comprehensive testing combines multiple approaches for thorough validation. Unit tests verify individual components. Integration tests validate system interactions. Property-based tests explore edge cases. Performance benchmarks ensure scalability.

Create robust testing suites using these patterns:

```rust
use aura_testkit::{ProtocolTestFixture, TestExecutionMode, StatelessFixtureConfig, ChoreographyTestHarness};

pub struct ComprehensiveTestSuite {
    protocol_name: String,
    config: StatelessFixtureConfig,
    unit_tests: Vec<UnitTestCase>,
    integration_tests: Vec<IntegrationTestCase>,
    property_tests: Vec<PropertyTestCase>,
    benchmarks: Vec<BenchmarkCase>,
}

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

    pub async fn run_comprehensive_protocol_validation(
        &mut self,
    ) -> Result<ValidationReport, TestError> {
        // Discover and setup test cases using stateless architecture
        self.discover_unit_tests().await?;
        self.create_integration_scenarios().await?;
        self.define_property_tests().await?;
        self.create_performance_benchmarks().await?;

        let mut validation_report = ValidationReport::new(&self.protocol_name);

        // Run unit tests with stateless fixtures
        for unit_test in &self.unit_tests {
            let fixture = ProtocolTestFixture::with_stateless_effects(
                self.config.threshold,
                self.config.total_devices,
                TestExecutionMode::UnitTest,
                self.config.seed,
            ).await?;

            let result = unit_test.execute(&fixture).await?;
            validation_report.add_unit_test_result(result);
        }

        // Run integration tests with choreography harness
        for integration_test in &self.integration_tests {
            let fixtures = create_multi_device_fixtures(
                integration_test.device_count(),
                &self.config,
                TestExecutionMode::Integration,
            ).await?;

            let harness = ChoreographyTestHarness::from_fixtures(
                fixtures,
                TestExecutionMode::Integration,
            ).await?;

            let result = integration_test.execute(&harness).await?;
            validation_report.add_integration_test_result(result);
        }

        // Run property-based tests
        for property_test in &self.property_tests {
            let simulation_fixtures = create_multi_device_fixtures(
                property_test.device_count(),
                &self.config,
                TestExecutionMode::Simulation,
            ).await?;

            let result = property_test.execute(&simulation_fixtures).await?;
            validation_report.add_property_test_result(result);
        }

        // Run performance benchmarks
        for benchmark in &self.benchmarks {
            let benchmark_fixtures = create_multi_device_fixtures(
                benchmark.device_count(),
                &self.config,
                TestExecutionMode::Simulation,
            ).await?;

            let result = benchmark.execute(&benchmark_fixtures).await?;
            validation_report.add_benchmark_result(result);
        }

        Ok(validation_report)
    }

    async fn discover_unit_tests(&mut self) -> Result<(), TestError> {
        self.unit_tests = discover_unit_tests(&self.protocol_name)?;
        Ok(())
    }

    async fn create_integration_scenarios(&mut self) -> Result<(), TestError> {
        self.integration_tests = create_integration_scenarios(&self.protocol_name)?;
        Ok(())
    }

    async fn define_property_tests(&mut self) -> Result<(), TestError> {
        self.property_tests = define_protocol_properties(&self.protocol_name)?;
        Ok(())
    }

    async fn create_performance_benchmarks(&mut self) -> Result<(), TestError> {
        self.benchmarks = create_performance_benchmarks(&self.protocol_name)?;
        Ok(())
    }
}

/// Helper function to create multiple fixtures for multi-device tests
async fn create_multi_device_fixtures(
    device_count: usize,
    base_config: &StatelessFixtureConfig,
    execution_mode: TestExecutionMode,
) -> Result<Vec<ProtocolTestFixture>, TestError> {
    let mut fixtures = Vec::new();
    for i in 0..device_count {
        let config = StatelessFixtureConfig {
            execution_mode,
            seed: base_config.seed + i as u64,
            total_devices: device_count as u16,
            ..base_config.clone()
        };
        let fixture = ProtocolTestFixture::with_stateless_effects(
            config.threshold,
            config.total_devices,
            config.execution_mode,
            config.seed,
        ).await?;
        fixtures.push(fixture);
    }
    Ok(fixtures)
}
```

Test suite composition provides systematic validation coverage. Automated test discovery reduces maintenance overhead. The stateless architecture enables clean separation between different test types.

Testing provides confidence in distributed protocol correctness. Property-based testing validates fundamental invariants. Integration testing ensures system-level functionality. Performance benchmarking validates scalability requirements.

For simulation capabilities that enable comprehensive fault injection and deterministic execution, see [Simulation Guide](806_simulation_guide.md). Learn effect system details in [Effects API](500_effects_api.md). Explore semilattice operations in [Semilattice API](501_semilattice_api.md). Review choreography syntax in [Choreography API](502_choreography_api.md).