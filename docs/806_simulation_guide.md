# Simulation Guide

This guide covers Aura's simulation engine built on the testkit infrastructure and stateless effect system architecture. The simulation engine provides deterministic, reproducible testing of distributed protocols through effect injection and fault simulation.

## Core Simulation Philosophy

Aura's simulation approach is built on three key principles. Tests run the actual `DeviceAgent` and protocol logic rather than mocks. All randomness and timing is controlled for 100% reproducible bugs. Fault injection happens at the same boundaries as production side effects.

This approach unlocks testing capabilities far superior to traditional integration testing. The simulation leverages the injectable effect interfaces throughout Aura's architecture for comprehensive validation.

## Simulation Infrastructure

Aura's simulation system enables controlled testing of distributed protocols using a stateless effect system. The simulation provides deterministic environments for protocol validation with configurable network conditions and failure scenarios.

### Testkit Integration

The simulation engine builds directly on the testkit infrastructure. All participants use `ProtocolTestFixture` instances created through `TestEffectsBuilder` for consistent behavior. The testkit provides the foundation for stateless fixture creation and effect system management.

```rust
use aura_testkit::{TestEffectsBuilder, TestExecutionMode, StatelessFixtureConfig};

pub struct ProtocolSimulation {
    fixtures: Vec<ProtocolTestFixture>,
    config: SimulationConfig,
}

impl ProtocolSimulation {
    pub async fn new(device_count: usize, config: SimulationConfig) -> Result<Self, SimulationError> {
        let mut fixtures = Vec::new();

        for i in 0..device_count {
            let device_id = aura_core::DeviceId::new();
            let effects_builder = TestEffectsBuilder::for_simulation(device_id)
                .with_seed(config.base_seed + i as u64);

            let fixture = ProtocolTestFixture::from_effects_builder(
                effects_builder,
                config.threshold,
                device_count as u16,
            ).await?;

            fixtures.push(fixture);
        }

        Ok(Self {
            fixtures,
            config,
        })
    }
}
```

The simulation creates multiple `ProtocolTestFixture` instances using the testkit infrastructure. Each fixture uses a deterministic seed derived from the base configuration. This ensures reproducible behavior across simulation runs.

### Core Simulation Components

The simulation engine consists of several key components that work together to manage the simulated world:

#### The Simulation Harness

```rust
use aura_testkit::{TestEffectsBuilder, TestExecutionMode, StatelessFixtureConfig};
use aura_simulator::{SimulationEngine, EffectInterceptor, EffectContext};

/// Main simulation harness that owns the entire simulated world
pub struct SimulationEngine {
    /// Central runtime that intercepts and processes side effects
    effect_runtime: StatelessEffectRuntime,
    /// All participants in the simulation
    participants: HashMap<DeviceId, SimulatedParticipant>,
    /// Deterministic time source
    time_source: SimulatedTime,
    /// Centralized network simulation
    network: SimulatedNetwork,
    /// Global simulation seed for reproducibility
    seed: u64,
}

impl SimulationEngine {
    pub fn new(seed: u64) -> Self {
        Self {
            effect_runtime: StatelessEffectRuntime::new(seed),
            participants: HashMap::new(),
            time_source: SimulatedTime::new(0),
            network: SimulatedNetwork::new(seed),
            seed,
        }
    }

    /// Add an honest participant to the simulation
    pub async fn add_participant(&mut self, device_id: DeviceId) -> Result<&SimulatedParticipant, SimulationError> {
        let config = StatelessFixtureConfig {
            execution_mode: TestExecutionMode::Simulation,
            seed: self.seed + device_id.as_u64(),
            threshold: 2,
            total_devices: 3,
            primary_device: Some(device_id),
        };

        let participant = SimulatedParticipant::new(
            device_id,
            config,
            self.effect_runtime.effect_sink_for(device_id),
        ).await?;

        self.participants.insert(device_id, participant);
        Ok(self.participants.get(&device_id).unwrap())
    }

    /// Add a malicious participant with effect interception capabilities
    pub async fn add_malicious_participant(
        &mut self,
        device_id: DeviceId,
        interceptor: Box<dyn EffectInterceptor>,
    ) -> Result<&MaliciousParticipant, SimulationError> {
        // Implementation for Byzantine participants
        todo!()
    }

    /// Convenience helper for tests to inspect participant state
    pub fn ledger_snapshot(&self, participant: DeviceId) -> Result<AccountState, SimulationError> {
        self.participants
            .get(&participant)
            .ok_or(SimulationError::ParticipantNotFound(participant))?
            .account_state()
    }

    /// Advance simulation by one tick
    pub async fn tick(&mut self) -> Result<(), SimulationError> {
        self.time_source.advance(1);
        self.network.process_tick(self.time_source.current_time()).await?;
        self.deliver_pending_messages().await
    }

    /// Run simulation until no more messages are in flight
    pub async fn run_until_idle(&mut self) -> Result<(), SimulationError> {
        while self.network.has_pending_messages() {
            self.tick().await?;
        }
        Ok(())
    }
}
```

The simulation engine owns the entire simulated world and provides the API for test scripts. Each participant is created using the testkit infrastructure with a `StatelessFixtureConfig`. The engine coordinates time advancement and message delivery across all participants.

#### Simulated Participants

```rust
/// Wrapper around a real DeviceAgent with injected simulation components
pub struct SimulatedParticipant {
    device_id: DeviceId,
    fixture: ProtocolTestFixture,
    effects_builder: TestEffectsBuilder,
    effect_sink: EffectSink,
}

impl SimulatedParticipant {
    pub async fn new(
        device_id: DeviceId,
        config: StatelessFixtureConfig,
        effect_sink: EffectSink,
    ) -> Result<Self, SimulationError> {
        let effects_builder = TestEffectsBuilder::for_simulation(device_id)
            .with_seed(config.seed);

        let fixture = ProtocolTestFixture::from_effects_builder(
            effects_builder.clone(),
            config.threshold,
            config.total_devices,
        ).await?;

        Ok(Self {
            device_id,
            fixture,
            effects_builder,
            effect_sink,
        })
    }

    /// Get current account state for inspection
    pub fn account_state(&self) -> Result<AccountState, SimulationError> {
        Ok(self.fixture.account_state().clone())
    }

    /// Execute an action on this participant
    pub async fn execute_action(&mut self, action: Action) -> Result<ActionResult, SimulationError> {
        // Execute action through stateless effects and emit results to effect_sink
        todo!()
    }
}
```

Each simulated participant wraps a real protocol implementation with injected simulation components. The participant uses a `ProtocolTestFixture` from the testkit for consistent state management. Actions execute through the stateless effect system and emit results to the central runtime.

#### Effect Interception and Fault Injection

```rust
/// Context provided to effect interceptors
#[derive(Debug, Clone)]
pub struct EffectContext {
    pub operation: Operation,
    pub sender: DeviceId,
    pub recipients: Vec<DeviceId>,
    pub tick: u64,
    pub protocol_phase: Option<ProtocolPhase>,
}

/// Trait for intercepting and modifying effects (for fault injection)
pub trait EffectInterceptor: Send + Sync {
    /// Intercept outgoing effects before they leave the participant
    /// Returning Some(effect) forwards (possibly modified) effect
    /// Returning None drops the effect
    fn intercept_outgoing(
        &self,
        ctx: &EffectContext,
        effect: Effect,
    ) -> Option<Effect>;

    /// Intercept incoming effects before the participant sees them
    fn intercept_incoming(
        &self,
        ctx: &EffectContext,
        effect: Effect,
    ) -> Option<Effect>;
}

/// Runtime for processing effects in the simulation
pub struct StatelessEffectRuntime {
    network: SimulatedNetwork,
    scheduler: TickScheduler,
    interceptors: HashMap<DeviceId, Box<dyn EffectInterceptor>>,
    seed: u64,
}

impl StatelessEffectRuntime {
    pub fn new(seed: u64) -> Self {
        Self {
            network: SimulatedNetwork::new(seed),
            scheduler: TickScheduler::new(),
            interceptors: HashMap::new(),
            seed,
        }
    }

    /// Register an effect interceptor for a specific participant
    pub fn register_interceptor(
        &mut self,
        device_id: DeviceId,
        interceptor: Box<dyn EffectInterceptor>,
    ) {
        self.interceptors.insert(device_id, interceptor);
    }

    /// Process an effect from a participant
    pub async fn process_effect(
        &mut self,
        ctx: EffectContext,
        effect: Effect,
    ) -> Result<(), SimulationError> {
        // Apply outgoing interception
        let effect = if let Some(interceptor) = self.interceptors.get(&ctx.sender) {
            match interceptor.intercept_outgoing(&ctx, effect) {
                Some(modified_effect) => modified_effect,
                None => return Ok(()), // Effect was dropped
            }
        } else {
            effect
        };

        // Route effect based on type
        match effect {
            Effect::Send(envelope) => {
                self.network.enqueue_message(ctx.tick, envelope).await?
            }
            Effect::WriteToLocalLedger(event) => {
                // Send back to originating participant
                self.deliver_to_participant(ctx.sender, Effect::WriteToLocalLedger(event)).await?
            }
            Effect::UpdateTime(new_time) => {
                // Update global time source
                self.scheduler.set_time(new_time);
            }
            _ => {
                // Handle other effect types
            }
        }

        Ok(())
    }
}
```

Effect interception provides sophisticated fault injection capabilities. Interceptors operate on the same effect boundaries as production code. The runtime processes effects from all participants and routes them appropriately through the simulated network.

#### Simulated Network

```rust
/// Network simulation with latency, partitions, and message delivery
pub struct SimulatedNetwork {
    /// Messages scheduled for delivery, keyed by delivery tick
    inflight_messages: BTreeMap<u64, Vec<Envelope>>,
    /// Per-participant message inboxes
    peer_mailboxes: HashMap<DeviceId, VecDeque<Envelope>>,
    /// Network configuration
    latency_range: Range<u64>,
    partitions: Vec<HashSet<DeviceId>>,
    /// Deterministic RNG for network behavior
    rng: ChaCha8Rng,
}

impl SimulatedNetwork {
    pub fn new(seed: u64) -> Self {
        Self {
            inflight_messages: BTreeMap::new(),
            peer_mailboxes: HashMap::new(),
            latency_range: 1..10,
            partitions: Vec::new(),
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    /// Configure network latency range
    pub fn set_latency_range(&mut self, range: Range<u64>) {
        self.latency_range = range;
    }

    /// Add a network partition
    pub fn add_partition(&mut self, partition: HashSet<DeviceId>) {
        self.partitions.push(partition);
    }

    /// Enqueue a message for delivery with simulated latency
    pub async fn enqueue_message(
        &mut self,
        current_tick: u64,
        envelope: Envelope,
    ) -> Result<(), SimulationError> {
        // Calculate delivery delay using deterministic RNG
        let delay = self.rng.gen_range(self.latency_range.clone());
        let delivery_tick = current_tick + delay;

        // Check if message is blocked by partitions
        if self.is_partitioned(&envelope.sender, &envelope.recipients) {
            // Drop message due to partition
            return Ok(());
        }

        // Schedule for delivery
        self.inflight_messages
            .entry(delivery_tick)
            .or_insert_with(Vec::new)
            .push(envelope);

        Ok(())
    }

    /// Process messages due for delivery at the current tick
    pub async fn process_tick(&mut self, current_tick: u64) -> Result<(), SimulationError> {
        if let Some(messages) = self.inflight_messages.remove(&current_tick) {
            for envelope in messages {
                for recipient in &envelope.recipients {
                    self.peer_mailboxes
                        .entry(*recipient)
                        .or_insert_with(VecDeque::new)
                        .push_back(envelope.clone());
                }
            }
        }
        Ok(())
    }

    pub fn has_pending_messages(&self) -> bool {
        !self.inflight_messages.is_empty() ||
        self.peer_mailboxes.values().any(|mailbox| !mailbox.is_empty())
    }

    fn is_partitioned(&self, sender: &DeviceId, recipients: &[DeviceId]) -> bool {
        for partition in &self.partitions {
            let sender_in_partition = partition.contains(sender);
            let any_recipient_outside = recipients.iter()
                .any(|r| !partition.contains(r));

            if sender_in_partition && any_recipient_outside {
                return true;
            }
        }
        false
    }
}
```

The simulated network provides realistic message delivery with configurable latency and partitions. All behavior uses deterministic randomness for reproducible results. Messages are scheduled for delivery based on simulated network conditions.

### Network Simulation

Configure network behavior using the stateless effect system:

```rust
use aura_simulator::middleware::stateless_effects::StatelessEffectsMiddleware;

pub fn create_realistic_network_conditions() -> StatelessFixtureConfig {
    StatelessFixtureConfig {
        execution_mode: TestExecutionMode::Simulation,
        seed: 42,
        threshold: 2,
        total_devices: 5,
        primary_device: None,
    }
}

pub async fn simulate_network_partition(
    simulator: &mut SimulatorStackBuilder,
    partition_devices: Vec<aura_core::DeviceId>,
    duration: std::time::Duration,
) -> Result<(), SimulationError> {
    // Use stateless middleware to inject network faults
    let fault_middleware = StatelessEffectsMiddleware::new()
        .with_network_partition(partition_devices.clone(), duration);

    simulator.add_middleware(fault_middleware).await?;

    // Execute protocol under partition conditions
    let simulation_result = simulator.execute_simulation().await?;

    Ok(())
}
```

Network conditions simulate real-world environments with latency and packet loss. Partitions test protocol behavior during network splits. Bandwidth limits validate performance under constrained conditions.

### Device Failure Simulation

Test protocol resilience with device failures using stateless effects:

```rust
use aura_testkit::{TestEffectsBuilder, TestExecutionMode};

pub async fn simulate_cascading_failures(
    simulation: &mut ProtocolSimulation,
    initial_failure_device: aura_core::DeviceId,
    failure_spread_rate: f64,
) -> Result<FailureAnalysis, SimulationError> {
    let mut failed_devices = vec![initial_failure_device];
    let mut failure_timeline = Vec::new();

    // Create failure-aware effect systems
    for fixture in &mut simulation.fixtures {
        if fixture.device_id() == initial_failure_device {
            // Replace with failed effect system
            let failed_builder = TestEffectsBuilder::for_simulation(fixture.device_id())
                .with_seed(fixture.seed())
                .with_mock_network(true); // Network failure simulation

            // Update fixture to use failed effects
            *fixture = ProtocolTestFixture::from_effects_builder(
                failed_builder,
                simulation.config.threshold,
                simulation.config.total_devices,
            ).await?;
        }
    }

    failure_timeline.push((get_current_simulation_time(), initial_failure_device));

    // Simulate failure spread using stateless approach
    while failed_devices.len() < simulation.fixtures.len() / 2 {
        let propagation_delay = calculate_failure_propagation_delay(failure_spread_rate);
        tokio::time::sleep(propagation_delay).await;

        let next_failure = select_next_failure_target(&simulation.fixtures, &failed_devices)?;

        // Update next device with failed effect system
        if let Some(fixture) = simulation.fixtures.iter_mut()
            .find(|f| f.device_id() == next_failure) {

            let failed_builder = TestEffectsBuilder::for_simulation(fixture.device_id())
                .with_seed(fixture.seed())
                .with_mock_network(true);

            *fixture = ProtocolTestFixture::from_effects_builder(
                failed_builder,
                simulation.config.threshold,
                simulation.config.total_devices,
            ).await?;
        }

        failed_devices.push(next_failure);
        failure_timeline.push((get_current_simulation_time(), next_failure));
    }

    Ok(FailureAnalysis {
        total_failed: failed_devices.len(),
        failure_timeline,
        system_stability: assess_system_stability_stateless(simulation).await?,
    })
}
```

Failure simulation tests system resilience under various failure scenarios. Cascading failures validate graceful degradation patterns. Failure analysis provides metrics for system robustness assessment.

## Deterministic Simulation Examples

The following examples demonstrate how to use the simulation engine for comprehensive protocol testing:

### Byzantine Fault Tolerance Testing

```rust
use aura_simulator::{SimulationEngine, EffectInterceptor, EffectContext, Action};
use aura_testkit::{StatelessFixtureConfig, TestExecutionMode};

#[tokio::test]
async fn test_byzantine_resharing_is_aborted() {
    // 1. Setup: All randomness and time are controlled by the simulation
    let mut sim = SimulationEngine::new(42); // Deterministic seed

    let alice = DeviceId::new();
    let byzantine_bob = DeviceId::new();
    let carol = DeviceId::new();

    // Add honest participants
    sim.add_participant(alice).await.unwrap();
    sim.add_participant(carol).await.unwrap();

    // 2. Define malicious behavior through effect interception
    struct ResharingCorruptor;
    impl EffectInterceptor for ResharingCorruptor {
        fn intercept_outgoing(&self, ctx: &EffectContext, effect: Effect) -> Option<Effect> {
            match effect {
                Effect::Send(mut envelope) if ctx.operation == Operation::ProduceResharingSubShare => {
                    // Corrupt the resharing subshare
                    envelope.payload = generate_corrupted_subshare();
                    Some(Effect::Send(envelope))
                }
                _ => Some(effect), // Forward all other effects unchanged
            }
        }

        fn intercept_incoming(&self, _ctx: &EffectContext, effect: Effect) -> Option<Effect> {
            Some(effect) // Don't modify incoming effects
        }
    }

    // Add malicious participant with interceptor
    sim.add_malicious_participant(byzantine_bob, Box::new(ResharingCorruptor)).await.unwrap();

    // 3. Script the scenario
    let action = Action::InitiateResharing {
        new_participants: vec![alice, byzantine_bob, carol],
        threshold: 2,
    };
    sim.tell_participant(alice, action).await.unwrap();

    // 4. Run the simulation until completion
    sim.run_until_idle().await.unwrap();

    // 5. Assert final state
    let alice_state = sim.ledger_snapshot(alice).unwrap();
    let last_event = alice_state.get_last_event();
    assert_matches!(
        last_event.payload, 
        EventPayload::ResharingAborted { 
            reason: AbortReason::InvalidShare 
        }
    );
    
    // Verify Byzantine participant is blamed
    assert!(alice_state.is_device_blamed(byzantine_bob));
}
```

This test demonstrates deterministic Byzantine fault injection through effect interception. The simulation runs the actual protocol logic with a corrupted participant. The test verifies that honest participants detect and handle the Byzantine behavior correctly.

### Network Partition Recovery

```rust
#[tokio::test]
async fn test_recovery_after_network_partition() {
    let mut sim = SimulationEngine::new(123);
    
    // Create a 3-of-5 threshold setup
    let devices: Vec<DeviceId> = (0..5).map(|_| DeviceId::new()).collect();
    for device in &devices {
        sim.add_participant(*device).await.unwrap();
    }
    
    // Initial state: all devices can communicate
    let action = Action::InitiateThresholdSigning {
        message: b"test message".to_vec(),
        signers: devices.clone(),
    };
    sim.tell_participant(devices[0], action).await.unwrap();
    
    // Let protocol make some progress
    for _ in 0..10 {
        sim.tick().await.unwrap();
    }
    
    // Introduce network partition: isolate 2 devices
    let partition_a: HashSet<_> = devices[..3].iter().cloned().collect();
    let partition_b: HashSet<_> = devices[3..].iter().cloned().collect();
    
    sim.network_mut().add_partition(partition_a.clone());
    sim.network_mut().add_partition(partition_b.clone());
    
    // Run during partition
    for _ in 0..20 {
        sim.tick().await.unwrap();
    }
    
    // Remove partition
    sim.network_mut().clear_partitions();
    
    // Allow recovery
    sim.run_until_idle().await.unwrap();
    
    // Verify all devices converged to the same state
    let reference_state = sim.ledger_snapshot(devices[0]).unwrap();
    for device in &devices[1..] {
        let device_state = sim.ledger_snapshot(*device).unwrap();
        assert_eq!(reference_state.merkle_root(), device_state.merkle_root());
    }
}
```

This test demonstrates network partition simulation and recovery validation. The simulation partitions the network during protocol execution. After partition removal the test verifies that all participants converge to a consistent state.

### Complex Multi-Phase Protocol

```rust
#[tokio::test]
async fn test_complete_account_recovery_workflow() {
    let mut sim = SimulationEngine::new(456);
    
    // Setup: Alice's device is compromised, guardians help recovery
    let alice_old = DeviceId::new();
    let alice_new = DeviceId::new();
    let guardian1 = DeviceId::new();
    let guardian2 = DeviceId::new();
    let guardian3 = DeviceId::new();
    
    let guardians = vec![guardian1, guardian2, guardian3];
    
    // Add all participants
    sim.add_participant(alice_old).await.unwrap();
    sim.add_participant(alice_new).await.unwrap();
    for guardian in &guardians {
        sim.add_participant(*guardian).await.unwrap();
    }
    
    // Configure network with realistic latency
    sim.network_mut().set_latency_range(5..50); // 5-50 ticks delay
    
    // Phase 1: Alice initiates recovery from new device
    let recovery_action = Action::InitiateAccountRecovery {
        old_device: alice_old,
        new_device: alice_new,
        guardians: guardians.clone(),
        recovery_threshold: 2, // 2-of-3 guardians needed
    };
    sim.tell_participant(alice_new, recovery_action).await.unwrap();
    
    // Phase 2: Guardians respond (with one guardian being slow)
    sim.run_for_ticks(100).await.unwrap(); // Allow initial propagation
    
    // Phase 3: Simulate slow guardian by introducing temporary partition
    let slow_guardian_partition = [guardian3].iter().cloned().collect();
    sim.network_mut().add_partition(slow_guardian_partition);
    
    sim.run_for_ticks(200).await.unwrap(); // Recovery should complete with 2 guardians
    
    // Phase 4: Slow guardian rejoins
    sim.network_mut().clear_partitions();
    sim.run_until_idle().await.unwrap();
    
    // Verify recovery succeeded
    let alice_new_state = sim.ledger_snapshot(alice_new).unwrap();
    assert!(alice_new_state.has_device_access(alice_new));
    assert!(!alice_new_state.has_device_access(alice_old));
    
    // Verify all guardians have consistent view
    let guardian_states: Vec<_> = guardians.iter()
        .map(|g| sim.ledger_snapshot(*g).unwrap())
        .collect();
    
    let reference_root = guardian_states[0].merkle_root();
    for state in &guardian_states[1..] {
        assert_eq!(state.merkle_root(), reference_root);
    }
    
    // Verify timing properties
    let recovery_events = alice_new_state.events_of_type(EventType::AccountRecoveryCompleted);
    assert_eq!(recovery_events.len(), 1);
    let completion_time = recovery_events[0].timestamp;
    assert!(completion_time < 400); // Should complete within reasonable time
}
```

This test demonstrates a complex multi-phase protocol with realistic network conditions. The simulation handles multiple participants with different roles. The test validates both correctness and timing properties of the recovery protocol.

## Advanced Simulation Techniques

### Deterministic Byzantine Behavior Modeling

The simulation engine supports sophisticated Byzantine fault injection through effect interception:

```rust
use aura_simulator::{SimulationEngine, EffectInterceptor, EffectContext};

/// Interceptor that selectively corrupts protocol messages
struct ProtocolCorruptorInterceptor {
    corruption_probability: f64,
    target_operations: HashSet<Operation>,
    rng: ChaCha8Rng,
}

impl EffectInterceptor for ProtocolCorruptorInterceptor {
    fn intercept_outgoing(&self, ctx: &EffectContext, effect: Effect) -> Option<Effect> {
        if self.target_operations.contains(&ctx.operation) {
            if self.rng.gen::<f64>() < self.corruption_probability {
                return Some(self.corrupt_based_on_operation(ctx, effect));
            }
        }
        Some(effect)
    }

    fn intercept_incoming(&self, _ctx: &EffectContext, effect: Effect) -> Option<Effect> {
        Some(effect) // Don't corrupt incoming messages
    }
}

impl ProtocolCorruptorInterceptor {
    fn corrupt_based_on_operation(&self, ctx: &EffectContext, effect: Effect) -> Effect {
        match (&ctx.operation, effect) {
            (Operation::ThresholdSign, Effect::Send(mut envelope)) => {
                // Corrupt threshold signature shares
                envelope.payload = self.generate_invalid_signature_share(&envelope.payload);
                Effect::Send(envelope)
            }
            (Operation::ConsensusVote, Effect::Send(mut envelope)) => {
                // Send conflicting votes to different participants
                envelope.payload = self.generate_conflicting_vote(&ctx.recipients, &envelope.payload);
                Effect::Send(envelope)
            }
            (Operation::StateSync, Effect::Send(mut envelope)) => {
                // Provide stale or corrupted state information
                envelope.payload = self.corrupt_state_sync(&envelope.payload);
                Effect::Send(envelope)
            }
            _ => effect,
        }
    }
}
```

This interceptor demonstrates sophisticated Byzantine behavior modeling. Different operations receive different types of corruption. The interceptor uses deterministic randomness to ensure reproducible Byzantine behavior.

### Simulation Debugging and Analysis

The simulation engine provides comprehensive debugging capabilities:

```rust
use aura_simulator::{TraceAnalyzer, ExecutionTrace, PerformanceProfiler};

#[tokio::test]
async fn test_with_comprehensive_analysis() {
    let mut sim = SimulationEngine::new(7000);
    sim.enable_detailed_tracing().await.unwrap();
    sim.enable_performance_profiling().await.unwrap();
    
    // ... run simulation ...
    
    let trace = sim.get_execution_trace().await.unwrap();
    let analyzer = TraceAnalyzer::new(trace);
    
    // Analyze message flow patterns
    let message_flow = analyzer.analyze_message_flow();
    println!("Total messages: {}", message_flow.total_messages);
    println!("Average latency: {:.2} ticks", message_flow.average_latency);
    println!("Message drops: {}", message_flow.dropped_messages);
    
    // Analyze protocol phases
    let phases = analyzer.identify_protocol_phases();
    for phase in phases {
        println!("Phase {}: {} ticks ({}->{})", 
                phase.name, 
                phase.duration, 
                phase.start_tick, 
                phase.end_tick);
    }
    
    // Identify performance bottlenecks
    let bottlenecks = analyzer.identify_performance_bottlenecks();
    for bottleneck in bottlenecks {
        println!("Bottleneck: {} at tick {} (devices: {:?})", 
                bottleneck.description,
                bottleneck.tick,
                bottleneck.affected_devices);
    }
    
    // Generate performance report
    let profiler = sim.get_performance_profiler().unwrap();
    let perf_report = profiler.generate_report();
    
    println!("Performance Summary:");
    println!("  CPU usage: {:.2}%", perf_report.cpu_utilization);
    println!("  Memory peak: {} MB", perf_report.peak_memory_mb);
    println!("  Network bandwidth: {} KB/s", perf_report.avg_network_bandwidth_kb);
}
```

The simulation provides detailed tracing and performance analysis capabilities. Message flow analysis identifies communication patterns and bottlenecks. Protocol phase identification helps understand execution flow and timing.

## Conclusion

Aura's simulation infrastructure represents a paradigm shift in distributed systems testing. By building on injectable effect interfaces and deterministic execution the simulation engine enables comprehensive testing capabilities.

### Key Capabilities

Production code testing runs actual protocol logic rather than mocks. Perfect reproducibility ensures 100% reliable bug reproduction. Sophisticated fault injection models Byzantine behavior through effect interception. Comprehensive analysis provides deep insights into protocol execution.

### Integration with Testkit

The simulation builds directly on the testkit infrastructure for consistency. All participants use `ProtocolTestFixture` instances for state management. The `TestEffectsBuilder` provides the foundation for stateless fixture creation. This integration ensures that simulation testing aligns with other testing approaches.

### Testing Philosophy

This approach eliminates traditional trade-offs in distributed systems testing. Production code runs unmodified without mock complexity. Perfect reproducibility eliminates non-determinism. Full protocol complexity is preserved without simplified models.

The simulation infrastructure positions Aura to build robust distributed systems through comprehensive testing of production code under all possible conditions.

For comprehensive testing infrastructure that complements simulation capabilities see [Testing Guide](805_testing_guide.md). Learn effect system details in [Effects API](500_effects_api.md). Explore semilattice operations in [Semilattice API](501_semilattice_api.md). Review choreography syntax in [Choreography API](502_choreography_api.md).