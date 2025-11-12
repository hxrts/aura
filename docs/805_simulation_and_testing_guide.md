# Simulation and Testing Guide

This guide covers simulation and testing capabilities in Aura. You will learn comprehensive protocol testing approaches, simulation infrastructure, property-based testing, integration testing patterns, and performance benchmarking.

## Simulation Infrastructure

Aura's simulation system enables controlled testing of distributed protocols. The simulation provides deterministic environments for protocol validation with configurable network conditions and failure scenarios.

### Simulation Setup

Create simulation environments using the testkit infrastructure:

```rust
use aura_testkit::{SimulationBuilder, SimulatedNetwork, DeviceSimulator};

pub struct ProtocolSimulation {
    network: SimulatedNetwork,
    devices: Vec<DeviceSimulator>,
    config: SimulationConfig,
}

impl ProtocolSimulation {
    pub fn new(device_count: usize, config: SimulationConfig) -> Result<Self, SimulationError> {
        let mut builder = SimulationBuilder::new();
        
        for i in 0..device_count {
            let device_id = aura_core::DeviceId::new();
            builder.add_device(device_id, config.device_config.clone());
        }
        
        let network = builder.build_network(config.network_config)?;
        let devices = builder.create_devices()?;
        
        Ok(Self {
            network,
            devices,
            config,
        })
    }
}
```

Simulation builders create controlled environments with multiple virtual devices. Network simulators control message delivery timing and failure rates. Device simulators provide isolated execution contexts for protocols.

### Network Simulation

Configure network behavior for realistic testing:

```rust
use aura_testkit::NetworkConditions;

pub fn create_realistic_network_conditions() -> NetworkConditions {
    NetworkConditions {
        latency: LatencyModel::Normal {
            mean: std::time::Duration::from_millis(50),
            stddev: std::time::Duration::from_millis(20),
        },
        packet_loss: 0.01, // 1% packet loss
        bandwidth_limit: Some(1_000_000), // 1 MB/s
        partition_probability: 0.005, // 0.5% chance of temporary partition
    }
}

pub async fn simulate_network_partition(
    network: &mut SimulatedNetwork,
    partition_devices: Vec<aura_core::DeviceId>,
    duration: std::time::Duration,
) -> Result<(), SimulationError> {
    network.create_partition(partition_devices.clone()).await?;
    
    tokio::time::sleep(duration).await;
    
    network.heal_partition(partition_devices).await?;
    
    Ok(())
}
```

Network conditions simulate real-world environments with latency and packet loss. Partitions test protocol behavior during network splits. Bandwidth limits validate performance under constrained conditions.

### Device Failure Simulation

Test protocol resilience with device failures:

```rust
use aura_testkit::FailureMode;

pub async fn simulate_cascading_failures(
    simulation: &mut ProtocolSimulation,
    initial_failure_device: aura_core::DeviceId,
    failure_spread_rate: f64,
) -> Result<FailureAnalysis, SimulationError> {
    let mut failed_devices = vec![initial_failure_device];
    let mut failure_timeline = Vec::new();
    
    // Initial failure
    simulation.fail_device(initial_failure_device, FailureMode::Crash).await?;
    failure_timeline.push((simulation.current_time(), initial_failure_device));
    
    // Simulate failure spread
    while failed_devices.len() < simulation.device_count() / 2 {
        let propagation_delay = calculate_failure_propagation_delay(failure_spread_rate);
        tokio::time::sleep(propagation_delay).await;
        
        let next_failure = select_next_failure_target(&simulation.devices, &failed_devices)?;
        simulation.fail_device(next_failure, FailureMode::Crash).await?;
        
        failed_devices.push(next_failure);
        failure_timeline.push((simulation.current_time(), next_failure));
    }
    
    Ok(FailureAnalysis {
        total_failed: failed_devices.len(),
        failure_timeline,
        system_stability: assess_system_stability(simulation).await?,
    })
}
```

Failure simulation tests system resilience under various failure scenarios. Cascading failures validate graceful degradation patterns. Failure analysis provides metrics for system robustness assessment.

## Property-Based Testing

Property-based testing validates protocol correctness across diverse input spaces. Properties express invariants that must hold for all valid inputs and execution scenarios.

### Protocol Properties

Define fundamental properties for distributed protocols:

```rust
use proptest::prelude::*;
use aura_testkit::property_testing::{ProtocolProperty, PropertyResult};

pub struct ConsistencyProperty;

impl ProtocolProperty for ConsistencyProperty {
    async fn check_property(
        &self,
        simulation: &ProtocolSimulation,
        execution_trace: &ExecutionTrace,
    ) -> PropertyResult {
        let final_states = collect_final_device_states(simulation).await?;
        
        // Check state convergence
        let reference_state = &final_states[0];
        for state in &final_states[1..] {
            if !states_are_equivalent(reference_state, state) {
                return PropertyResult::Violated {
                    description: "State convergence failed".to_string(),
                    counterexample: format!("Reference: {:?}, Divergent: {:?}", reference_state, state),
                };
            }
        }
        
        // Check causal consistency
        for device_state in &final_states {
            if !validate_causal_ordering(&device_state.operation_log) {
                return PropertyResult::Violated {
                    description: "Causal consistency violated".to_string(),
                    counterexample: format!("Device state: {:?}", device_state),
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
        simulation: &ProtocolSimulation,
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
            let mut simulation = ProtocolSimulation::from_scenario(scenario)?;
            simulation.execute_protocol().await?;
            
            let trace = simulation.get_execution_trace();
            let consistency_property = ConsistencyProperty;
            
            assert_eq!(
                consistency_property.check_property(&simulation, &trace).await?,
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
use aura_testkit::integration::{IntegrationTestHarness, TestScenario};

/// Sealed supertrait for integration testing effects
pub trait IntegrationTestEffects: NetworkEffects + CryptoEffects + StorageEffects + TimeEffects + JournalEffects + ConsoleEffects {}
impl<T> IntegrationTestEffects for T where T: NetworkEffects + CryptoEffects + StorageEffects + TimeEffects + JournalEffects + ConsoleEffects {}

pub struct ThresholdSigningIntegrationTest {
    harness: IntegrationTestHarness,
    devices: Vec<aura_core::DeviceId>,
}

impl ThresholdSigningIntegrationTest {
    pub async fn setup(device_count: usize, threshold: usize) -> Result<Self, TestError> {
        let harness = IntegrationTestHarness::new().await?;
        let devices = harness.create_threshold_group(device_count, threshold).await?;
        
        Ok(Self { harness, devices })
    }
    
    pub async fn test_complete_signing_workflow(&self) -> Result<(), TestError> {
        let message = b"integration test message";
        
        // Phase 1: Initialize signing session
        let session_id = self.harness.initiate_threshold_signing(
            &self.devices,
            message,
        ).await?;
        
        // Phase 2: Collect partial signatures
        let partial_signatures = self.harness.collect_partial_signatures(
            session_id,
            &self.devices[..2], // Use threshold number of devices
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
            &self.devices,
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
pub async fn test_capability_journal_integration<E: IntegrationTestEffects>(
    effects: &E,
) -> Result<(), IntegrationError> {
    let device_id = aura_core::DeviceId::new();
    let initial_capabilities = create_test_capability_set();
    
    // Initialize journal with capabilities
    let journal = effects.initialize_journal(device_id, initial_capabilities.clone()).await?;
    
    // Perform capability-gated operation
    let operation = TreeOperation::AddMember {
        member_id: aura_core::DeviceId::new(),
        capabilities: create_restricted_capability_set(),
    };
    
    // Verify capability checking
    let capability_check = effects.check_operation_capability(
        &operation,
        &initial_capabilities,
    ).await?;
    
    assert!(capability_check.is_authorized());
    
    // Execute operation and update journal
    let updated_journal = effects.execute_operation(
        journal,
        operation,
        capability_check.authorization_proof(),
    ).await?;
    
    // Verify journal consistency
    assert!(updated_journal.version() > journal.version());
    assert!(updated_journal.contains_operation_evidence(&operation));
    
    Ok(())
}
```

Cross-layer validation tests ensure clean interfaces between system components. Capability and journal integration validates authorization and state management. Layer interactions must maintain security and consistency properties.

### Recovery and Resilience Testing

Test system recovery under various failure conditions:

```rust
pub struct RecoveryTestSuite {
    baseline_simulation: ProtocolSimulation,
    recovery_scenarios: Vec<RecoveryScenario>,
}

impl RecoveryTestSuite {
    pub async fn test_byzantine_fault_tolerance(&self) -> Result<ResilienceReport, TestError> {
        let byzantine_device_count = self.baseline_simulation.device_count() / 3; // f < n/3
        let mut reports = Vec::new();
        
        for scenario in &self.recovery_scenarios {
            let mut test_simulation = self.baseline_simulation.clone();
            
            // Introduce Byzantine devices
            let byzantine_devices = test_simulation.select_devices(byzantine_device_count);
            for device in &byzantine_devices {
                test_simulation.make_byzantine(*device, scenario.byzantine_behavior.clone()).await?;
            }
            
            // Execute protocol under Byzantine conditions
            let execution_result = test_simulation.execute_protocol_with_timeout(
                scenario.execution_timeout
            ).await;
            
            let scenario_report = RecoveryScenarioReport {
                scenario: scenario.clone(),
                byzantine_devices: byzantine_devices.clone(),
                execution_result,
                protocol_succeeded: execution_result.is_ok(),
                consistency_maintained: self.check_consistency_post_execution(&test_simulation).await?,
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
use aura_testkit::benchmarking::{BenchmarkHarness, PerformanceMetrics};

pub struct ProtocolBenchmark {
    harness: BenchmarkHarness,
    metrics_collector: MetricsCollector,
}

impl ProtocolBenchmark {
    pub async fn benchmark_threshold_signing(
        &self,
        device_counts: Vec<usize>,
        message_sizes: Vec<usize>,
        iterations: usize,
    ) -> Result<BenchmarkReport, BenchmarkError> {
        let mut results = Vec::new();
        
        for device_count in device_counts {
            for message_size in message_sizes {
                let mut iteration_results = Vec::new();
                
                for _ in 0..iterations {
                    let test_message = vec![0u8; message_size];
                    let devices = self.harness.create_devices(device_count).await?;
                    
                    let start_time = Instant::now();
                    
                    // Measure setup overhead
                    let setup_start = Instant::now();
                    let session_id = self.harness.setup_threshold_session(&devices).await?;
                    let setup_duration = setup_start.elapsed();
                    
                    // Measure signing latency
                    let signing_start = Instant::now();
                    let signature = self.harness.execute_threshold_signing(
                        session_id,
                        &test_message,
                        &devices[..2], // Threshold of 2
                    ).await?;
                    let signing_duration = signing_start.elapsed();
                    
                    // Measure verification latency
                    let verification_start = Instant::now();
                    let verification_result = self.harness.verify_signature(
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

### Scalability Analysis

Analyze protocol performance scaling characteristics:

```rust
pub struct ScalabilityAnalyzer {
    benchmark_harness: BenchmarkHarness,
}

impl ScalabilityAnalyzer {
    pub async fn analyze_communication_scaling(
        &self,
        max_device_count: usize,
    ) -> Result<ScalingReport, AnalysisError> {
        let device_counts: Vec<usize> = (1..=max_device_count).step_by(5).collect();
        let mut scaling_data = Vec::new();
        
        for device_count in device_counts {
            let benchmark_result = self.benchmark_harness.measure_broadcast_latency(
                device_count,
                1000, // 1KB message
            ).await?;
            
            let communication_overhead = self.analyze_communication_overhead(
                device_count,
                &benchmark_result,
            ).await?;
            
            scaling_data.push(ScalingDataPoint {
                device_count,
                latency_p50: benchmark_result.percentile(50.0),
                latency_p95: benchmark_result.percentile(95.0),
                latency_p99: benchmark_result.percentile(99.0),
                message_count: communication_overhead.total_messages,
                bandwidth_usage: communication_overhead.total_bandwidth,
                theoretical_minimum: calculate_theoretical_minimum(device_count),
            });
        }
        
        Ok(ScalingReport {
            protocol: "broadcast".to_string(),
            scaling_data,
            complexity_analysis: analyze_complexity_trends(&scaling_data),
            bottleneck_analysis: identify_scaling_bottlenecks(&scaling_data),
        })
    }
    
    fn analyze_complexity_trends(data: &[ScalingDataPoint]) -> ComplexityAnalysis {
        let latency_trend = fit_complexity_curve(&data.iter().map(|d| (d.device_count, d.latency_p50.as_nanos() as f64)).collect::<Vec<_>>());
        let bandwidth_trend = fit_complexity_curve(&data.iter().map(|d| (d.device_count, d.bandwidth_usage as f64)).collect::<Vec<_>>());
        
        ComplexityAnalysis {
            latency_complexity: latency_trend,
            bandwidth_complexity: bandwidth_trend,
            scaling_efficiency: calculate_efficiency_ratio(data),
        }
    }
}
```

Scalability analysis evaluates performance trends across different system sizes. Complexity analysis identifies algorithmic scaling characteristics. Bottleneck analysis helps prioritize optimization efforts.

### Memory and Resource Profiling

Profile resource usage during protocol execution:

```rust
use aura_testkit::profiling::{ResourceProfiler, MemorySnapshot};

pub struct ResourceBenchmark {
    profiler: ResourceProfiler,
}

impl ResourceBenchmark {
    pub async fn profile_journal_memory_usage(
        &self,
        operation_counts: Vec<usize>,
    ) -> Result<MemoryProfileReport, ProfilingError> {
        let mut profiles = Vec::new();
        
        for operation_count in operation_counts {
            let baseline_snapshot = self.profiler.take_memory_snapshot().await?;
            
            // Create journal and populate with operations
            let mut journal = aura_core::Journal::new(aura_core::DeviceId::new());
            
            let population_start = Instant::now();
            for i in 0..operation_count {
                let operation = create_test_operation(i);
                journal.apply_operation(operation)?;
                
                // Take periodic snapshots during population
                if i % 1000 == 0 {
                    let snapshot = self.profiler.take_memory_snapshot().await?;
                    profiles.push(MemoryDataPoint {
                        operation_count: i,
                        heap_usage: snapshot.heap_size,
                        stack_usage: snapshot.stack_size,
                        journal_size: journal.serialized_size(),
                        timestamp: population_start.elapsed(),
                    });
                }
            }
            
            let final_snapshot = self.profiler.take_memory_snapshot().await?;
            let memory_overhead = final_snapshot.heap_size - baseline_snapshot.heap_size;
            
            profiles.push(MemoryDataPoint {
                operation_count,
                heap_usage: final_snapshot.heap_size,
                stack_usage: final_snapshot.stack_size,
                journal_size: journal.serialized_size(),
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
use aura_testkit::TestSuite;

pub async fn run_comprehensive_protocol_validation(
    protocol_name: &str,
) -> Result<ValidationReport, TestError> {
    let mut test_suite = TestSuite::new(protocol_name);
    
    // Unit test coverage
    test_suite.add_unit_tests(discover_unit_tests(protocol_name)?).await?;
    
    // Integration test scenarios
    test_suite.add_integration_tests(create_integration_scenarios(protocol_name)?).await?;
    
    // Property-based testing
    test_suite.add_property_tests(define_protocol_properties(protocol_name)?).await?;
    
    // Performance benchmarks
    test_suite.add_benchmarks(create_performance_benchmarks(protocol_name)?).await?;
    
    let validation_result = test_suite.execute_all().await?;
    
    Ok(validation_result)
}
```

Comprehensive validation ensures protocol correctness across all dimensions. Test suite composition provides systematic validation coverage. Automated test discovery reduces maintenance overhead.

Testing and simulation provide confidence in distributed protocol correctness. Property-based testing validates fundamental invariants. Integration testing ensures system-level functionality. Performance benchmarking validates scalability requirements.

Continue with API reference documentation for detailed system interfaces. Learn effect system details in [Effects API](500_effects_api.md). Explore semilattice operations in [Semilattice API](501_semilattice_api.md). Review choreography syntax in [Choreography API](502_choreography_api.md).