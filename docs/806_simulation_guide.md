# Simulation Guide

This guide covers Aura's simulation engine built on the async-first effect system architecture. The simulation provides deterministic, reproducible testing of distributed protocols through effect interception, fault injection, and comprehensive network simulation - all compatible with WebAssembly environments.

## Core Simulation Philosophy

Aura's simulation approach is built on four key principles:

1. **Production Code Testing** - Run actual protocol implementations, not mocks or simplified models
2. **Deterministic Execution** - Controlled time, seeded randomness, and reproducible network behavior
3. **Effect Interception** - Fault injection at the same async effect boundaries as production
4. **WASM Compatibility** - All simulation features work in browser environments without OS threads

The simulation integrates with the `#[aura_test]` macro for seamless testing while providing advanced capabilities for complex distributed system validation.

## Simulation Infrastructure

Aura's simulation system provides controlled testing through async effect interception and comprehensive environment modeling.

### Async Effect System Integration

The simulation engine leverages the async effect system's context propagation and lifecycle management:

```rust
use aura_simulator::{SimulationBuilder, SimulationConfig};
use aura_testkit::{aura_test, freeze_time};

#[aura_test]
async fn simulate_protocol() {
    // Build simulation with async initialization
    let sim = SimulationBuilder::new()
        .with_device_count(5)
        .with_threshold(3)
        .with_deterministic_time()
        .with_effect_interception()
        .build()
        .await?;
    
    // Time is automatically controlled
    freeze_time();
    
    // Add participants with parallel initialization
    let devices = sim.add_participants(5).await?;
    
    // Configure network conditions
    sim.network()
        .add_latency(10..50)
        .add_packet_loss(0.01)
        .add_partition(vec![devices[0]], vec![devices[1..].to_vec()]);
    
    // Run simulation with automatic context propagation
    sim.run_until_idle().await?;
    
    // Verify convergence
    assert!(sim.all_devices_converged().await?);
}
```

The simulation integrates seamlessly with the `#[aura_test]` macro, providing automatic setup and deterministic execution.

### Core Simulation Components

The simulation engine uses modern async patterns with effect interception:

#### Async Simulation Engine

```rust
use aura_simulator::{AsyncSimulationEngine, EffectInterceptor, EffectContext};

/// Async simulation engine with effect interception
pub struct AsyncSimulationEngine {
    /// Effect system with interception capabilities
    effect_system: Arc<AuraEffectSystem>,
    /// Participant management with async lifecycle
    participants: Arc<RwLock<HashMap<DeviceId, SimulatedParticipant>>>,
    /// Deterministic time control (WASM-compatible)
    time_controller: TimeController,
    /// Network simulation with async operations
    network_sim: NetworkSimulator,
    /// Effect interceptor registry
    interceptors: Arc<RwLock<HashMap<DeviceId, Box<dyn EffectInterceptor>>>>,
    /// Lifecycle manager for clean shutdown
    lifecycle: LifecycleManager,
}

impl AsyncSimulationEngine {
    pub async fn new() -> Result<Self> {
        // Build effect system with simulation configuration
        let builder = AuraEffectSystemBuilder::new()
            .with_simulation_mode()
            .with_deterministic_time()
            .with_effect_interception();
        
        let effect_system = builder.build().await?;
        
        Ok(Self {
            effect_system: Arc::new(effect_system),
            participants: Arc::new(RwLock::new(HashMap::new())),
            time_controller: TimeController::new(),
            network_sim: NetworkSimulator::new(),
            interceptors: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: LifecycleManager::new(),
        })
    }

    /// Add participants with parallel initialization
    pub async fn add_participants(&self, count: usize) -> Result<Vec<DeviceId>> {
        let init_tasks: Vec<_> = (0..count)
            .map(|i| {
                let engine = self.clone();
                async move {
                    let device_id = DeviceId::new();
                    let participant = SimulatedParticipant::new(
                        device_id,
                        engine.effect_system.clone(),
                    ).await?;
                    
                    engine.participants.write().await
                        .insert(device_id, participant);
                    
                    Ok::<_, SimulationError>(device_id)
                }
            })
            .collect();
        
        // Parallel initialization (WASM-compatible)
        futures::future::try_join_all(init_tasks).await
    }

    /// Add Byzantine participant with custom effect interception
    pub async fn add_byzantine_participant(
        &self,
        interceptor: impl EffectInterceptor + 'static,
    ) -> Result<DeviceId> {
        let device_id = DeviceId::new();
        
        // Register interceptor
        self.interceptors.write().await
            .insert(device_id, Box::new(interceptor));
        
        // Create participant with interception enabled
        let participant = SimulatedParticipant::new(
            device_id,
            self.effect_system.clone(),
        ).await?;
        
        self.participants.write().await
            .insert(device_id, participant);
        
        Ok(device_id)
    }

    /// Get participant state snapshot
    pub async fn device_state(&self, device_id: DeviceId) -> Result<AccountState> {
        let participants = self.participants.read().await;
        let participant = participants.get(&device_id)
            .ok_or(SimulationError::ParticipantNotFound(device_id))?;
        
        Ok(participant.account_state().await)
    }

    /// Advance time with automatic message delivery
    pub async fn advance_time(&self, duration: Duration) -> Result<()> {
        self.time_controller.advance(duration);
        
        // Process any messages scheduled for delivery
        self.network_sim.process_scheduled_messages().await?;
        
        // Allow participants to react
        self.process_participant_actions().await
    }

    /// Run until network quiesces
    pub async fn run_until_idle(&self) -> Result<()> {
        let timeout = Duration::from_secs(30);
        let start = self.time_controller.now();
        
        while self.network_sim.has_pending_messages().await? {
            if self.time_controller.now() - start > timeout {
                return Err(SimulationError::Timeout);
            }
            
            self.advance_time(Duration::from_millis(10)).await?;
        }
        
        Ok(())
    }
}
```

The simulation engine owns the entire simulated world and provides the API for test scripts. Each participant is created using the testkit infrastructure with a `StatelessFixtureConfig`. The engine coordinates time advancement and message delivery across all participants.

#### Simulated Participants with Async Lifecycle

```rust
/// Participant with async effect system and lifecycle management
pub struct SimulatedParticipant {
    device_id: DeviceId,
    effect_system: Arc<AuraEffectSystem>,
    state: Arc<RwLock<AccountState>>,
    context: EffectContext,
    lifecycle: ParticipantLifecycle,
}

impl SimulatedParticipant {
    pub async fn new(
        device_id: DeviceId,
        effect_system: Arc<AuraEffectSystem>,
    ) -> Result<Self> {
        // Create participant context
        let context = EffectContext::new()
            .with_device_id(device_id)
            .with_flow_budget(10_000)
            .with_metadata("participant_type", "simulated");
        
        // Initialize with lifecycle management
        let lifecycle = ParticipantLifecycle::new();
        lifecycle.initialize().await?;
        
        Ok(Self {
            device_id,
            effect_system,
            state: Arc::new(RwLock::new(AccountState::new(device_id))),
            context,
            lifecycle,
        })
    }

    /// Execute action with context propagation
    pub async fn execute_action(&self, action: Action) -> Result<ActionResult> {
        // Execute within participant context
        with_context(self.context.clone(), async {
            match action {
                Action::InitiateProtocol { protocol, params } => {
                    self.execute_protocol(protocol, params).await
                }
                Action::RespondToMessage { message } => {
                    self.handle_message(message).await
                }
                _ => Err(SimulationError::UnsupportedAction),
            }
        }).await
    }
    
    /// Get current state snapshot
    pub async fn account_state(&self) -> AccountState {
        self.state.read().await.clone()
    }
    
    /// Shutdown participant cleanly
    pub async fn shutdown(&self) -> Result<()> {
        self.lifecycle.shutdown().await
    }
}
```

Participants use the full async effect system with proper lifecycle management and context propagation.

#### Async Effect Interception

```rust
/// Effect interceptor for async operations
#[async_trait]
pub trait EffectInterceptor: Send + Sync {
    /// Intercept async effect operations
    async fn intercept(
        &self,
        ctx: &EffectContext,
        operation: EffectOperation,
    ) -> InterceptResult {
        InterceptResult::Continue(operation)
    }
}

/// Interception results
pub enum InterceptResult {
    Continue(EffectOperation),     // Forward (possibly modified)
    Replace(EffectOperation),      // Replace with different operation  
    Block(SimulationError),        // Block with error
    Delay(Duration, EffectOperation), // Delay then forward
}

/// Byzantine behavior patterns
pub struct ByzantineInterceptor {
    corruption_rate: f64,
    delay_range: Range<u64>,
    drop_rate: f64,
}

#[async_trait]
impl EffectInterceptor for ByzantineInterceptor {
    async fn intercept(
        &self,
        ctx: &EffectContext,
        operation: EffectOperation,
    ) -> InterceptResult {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        
        // Drop messages
        if rng.gen::<f64>() < self.drop_rate {
            return InterceptResult::Block(
                SimulationError::MessageDropped
            );
        }
        
        // Delay messages
        if !self.delay_range.is_empty() {
            let delay = rng.gen_range(self.delay_range.clone());
            return InterceptResult::Delay(
                Duration::from_millis(delay),
                operation,
            );
        }
        
        // Corrupt messages
        if rng.gen::<f64>() < self.corruption_rate {
            if let EffectOperation::Send { mut envelope, .. } = operation {
                envelope.corrupt_payload();
                return InterceptResult::Replace(
                    EffectOperation::Send { envelope, .. }
                );
            }
        }
        
        InterceptResult::Continue(operation)
    }
}

#### Effect Processing with Async Interception

```rust
/// Async effect processor with interception pipeline
pub struct AsyncEffectProcessor {
    interceptors: Arc<RwLock<HashMap<DeviceId, Box<dyn EffectInterceptor>>>>,
    network_sim: Arc<NetworkSimulator>,
    time_controller: Arc<TimeController>,
    metrics: SimulationMetrics,
}

impl AsyncEffectProcessor {
    /// Process effect with full interception pipeline
    pub async fn process_effect(
        &self,
        ctx: EffectContext,
        operation: EffectOperation,
    ) -> Result<()> {
        // Apply interception if registered
        let final_operation = if let Some(interceptor) = 
            self.interceptors.read().await.get(&ctx.device_id) 
        {
            match interceptor.intercept(&ctx, operation).await {
                InterceptResult::Continue(op) => op,
                InterceptResult::Replace(op) => op,
                InterceptResult::Block(err) => {
                    self.metrics.record_blocked_operation();
                    return Err(err);
                }
                InterceptResult::Delay(duration, op) => {
                    // Schedule delayed delivery
                    self.time_controller.sleep(duration).await;
                    op
                }
            }
        } else {
            operation
        };
        
        // Route to appropriate handler
        match final_operation {
            EffectOperation::Send { envelope, recipient } => {
                self.network_sim.send_message(
                    ctx.device_id,
                    recipient,
                    envelope,
                ).await?
            }
            EffectOperation::Store { key, value } => {
                // Storage operations processed locally
                ctx.effects().storage()
                    .store_with_context(&ctx, key, value)
                    .await?
            }
            EffectOperation::Time { .. } => {
                // Time operations controlled by simulation
                self.time_controller.handle_time_operation(&ctx).await?
            }
        }
        
        self.metrics.record_processed_operation();
        Ok(())
    }
}

Effect interception provides sophisticated fault injection capabilities. Interceptors operate on the same effect boundaries as production code. The runtime processes effects from all participants and routes them appropriately through the simulated network.

#### Async Network Simulation

```rust
/// Network simulator with async operations and WASM compatibility
pub struct NetworkSimulator {
    /// Messages with scheduled delivery times
    scheduled_messages: Arc<RwLock<BTreeMap<Instant, Vec<ScheduledMessage>>>>,
    /// Active network conditions
    conditions: Arc<RwLock<NetworkConditions>>,
    /// Partition groups
    partitions: Arc<RwLock<Vec<(HashSet<DeviceId>, HashSet<DeviceId>)>>>,
    /// Metrics collection
    metrics: NetworkMetrics,
}

#[derive(Clone)]
pub struct NetworkConditions {
    pub latency_range: Range<u64>,
    pub jitter: u64,
    pub packet_loss: f64,
    pub bandwidth_limit: Option<u64>,
    pub reorder_rate: f64,
}

impl NetworkSimulator {
    pub fn new() -> Self {
        Self {
            scheduled_messages: Arc::new(RwLock::new(BTreeMap::new())),
            conditions: Arc::new(RwLock::new(NetworkConditions {
                latency_range: 1..10,
                jitter: 0,
                packet_loss: 0.0,
                bandwidth_limit: None,
                reorder_rate: 0.0,
            })),
            partitions: Arc::new(RwLock::new(Vec::new())),
            metrics: NetworkMetrics::new(),
        }
    }

    /// Configure network conditions fluently
    pub fn add_latency(&mut self, range: Range<u64>) -> &mut Self {
        self.conditions.write().await.latency_range = range;
        self
    }
    
    pub fn add_jitter(&mut self, jitter: u64) -> &mut Self {
        self.conditions.write().await.jitter = jitter;
        self
    }
    
    pub fn add_packet_loss(&mut self, rate: f64) -> &mut Self {
        self.conditions.write().await.packet_loss = rate;
        self
    }
    
    pub fn add_partition(
        &mut self, 
        group_a: Vec<DeviceId>, 
        group_b: Vec<DeviceId>
    ) -> &mut Self {
        let set_a: HashSet<_> = group_a.into_iter().collect();
        let set_b: HashSet<_> = group_b.into_iter().collect();
        self.partitions.write().await.push((set_a, set_b));
        self
    }

    /// Send message with network simulation
    pub async fn send_message(
        &self,
        sender: DeviceId,
        recipient: DeviceId,
        envelope: Envelope,
    ) -> Result<()> {
        let conditions = self.conditions.read().await;
        
        // Check packet loss
        if rand::thread_rng().gen::<f64>() < conditions.packet_loss {
            self.metrics.record_packet_drop();
            return Ok(()); // Message lost
        }
        
        // Check partitions
        if self.is_partitioned(sender, recipient).await {
            self.metrics.record_partition_drop();
            return Ok(()); // Message blocked
        }
        
        // Calculate delivery time with latency and jitter
        let base_latency = rand::thread_rng()
            .gen_range(conditions.latency_range.clone());
        let jitter = rand::thread_rng()
            .gen_range(0..conditions.jitter);
        let total_delay = Duration::from_millis(base_latency + jitter);
        
        let delivery_time = Instant::now() + total_delay;
        
        // Schedule message
        let scheduled = ScheduledMessage {
            sender,
            recipient,
            envelope,
            scheduled_at: Instant::now(),
        };
        
        self.scheduled_messages.write().await
            .entry(delivery_time)
            .or_insert_with(Vec::new)
            .push(scheduled);
        
        self.metrics.record_message_sent(total_delay);
        Ok(())
    }

    /// Process all messages ready for delivery
    pub async fn process_scheduled_messages(&self) -> Result<()> {
        let now = Instant::now();
        let mut messages = self.scheduled_messages.write().await;
        
        // Find all messages ready for delivery
        let ready_messages: Vec<_> = messages
            .range(..=now)
            .flat_map(|(_, msgs)| msgs.clone())
            .collect();
        
        // Remove delivered messages
        messages.retain(|time, _| time > &now);
        
        // Deliver messages
        for msg in ready_messages {
            self.deliver_message(msg).await?;
        }
        
        Ok(())
    }
    
    /// Check if messages are pending
    pub async fn has_pending_messages(&self) -> Result<bool> {
        Ok(!self.scheduled_messages.read().await.is_empty())
    }
    
    /// Check if devices are partitioned
    async fn is_partitioned(&self, sender: DeviceId, recipient: DeviceId) -> bool {
        let partitions = self.partitions.read().await;
        
        for (group_a, group_b) in partitions.iter() {
            let sender_in_a = group_a.contains(&sender);
            let sender_in_b = group_b.contains(&sender);
            let recipient_in_a = group_a.contains(&recipient);
            let recipient_in_b = group_b.contains(&recipient);
            
            // Partitioned if in different groups
            if (sender_in_a && recipient_in_b) || (sender_in_b && recipient_in_a) {
                return true;
            }
        }
        
        false
    }
}
```

The simulated network provides realistic message delivery with configurable latency and partitions. All behavior uses deterministic randomness for reproducible results. Messages are scheduled for delivery based on simulated network conditions.

### Network Simulation Patterns

The simulation provides rich network modeling capabilities:

```rust
use aura_testkit::{aura_test, NetworkSimulator};

#[aura_test]
async fn test_realistic_network_conditions() {
    let mut sim = NetworkSimulator::new();
    
    // Configure WAN-like conditions
    sim.add_latency(20..100)        // 20-100ms latency
        .add_jitter(10)              // ±10ms jitter
        .add_packet_loss(0.01)       // 1% packet loss
        .add_bandwidth_limit(10_000_000); // 10Mbps
    
    // Add asymmetric conditions
    sim.add_directional_latency(device1, device2, 50..60)
        .add_directional_latency(device2, device1, 100..150);
    
    // Simulate network storm
    sim.add_packet_storm(Duration::from_secs(5), 0.5); // 50% loss for 5s
    
    // Execute protocol
    let result = execute_protocol_with_network(&sim).await?;
    
    // Get network statistics
    let stats = sim.statistics();
    println!("Messages sent: {}", stats.total_sent);
    println!("Messages delivered: {}", stats.total_delivered);
    println!("Average latency: {:?}", stats.avg_latency);
    println!("Packet loss rate: {:.2}%", stats.loss_rate * 100.0);
}

#[aura_test]
async fn test_dynamic_network_changes() {
    let mut sim = NetworkSimulator::new();
    
    // Start with good conditions
    sim.add_latency(1..5);
    
    // Degrade over time
    let degradation_task = async {
        for i in 1..10 {
            advance_time_by(Duration::from_secs(10));
            sim.add_latency(i*10..(i+1)*10);
            sim.add_packet_loss(i as f64 * 0.01);
        }
    };
    
    // Run protocol while network degrades
    tokio::select! {
        result = execute_protocol() => {
            assert!(result.is_ok());
        }
        _ = degradation_task => {}
    }
}
```

### Device Failure Simulation

Test protocol resilience with sophisticated failure patterns:

```rust
use aura_testkit::{aura_test, FailureSimulator};

#[aura_test]
async fn test_cascading_failures() {
    let failure_sim = FailureSimulator::new();
    
    // Configure failure patterns
    failure_sim.add_failure_pattern(
        FailurePattern::Cascading {
            initial_device: device1,
            spread_rate: 0.3,      // 30% chance to spread per tick
            spread_delay: Duration::from_secs(5),
            max_failures: 2,       // Stop at 2 failed devices
        }
    );
    
    // Add correlated failures
    failure_sim.add_failure_pattern(
        FailurePattern::Correlated {
            trigger_device: device2,
            correlated_devices: vec![device3, device4],
            correlation_delay: Duration::from_secs(2),
        }
    );
    
    // Execute with failure injection
    let result = execute_protocol_with_failures(&failure_sim).await;
    
    // Analyze failure impact
    let analysis = failure_sim.analyze_impact();
    println!("Devices failed: {}", analysis.failed_devices.len());
    println!("Protocol completed: {}", analysis.protocol_succeeded);
    println!("Healthy devices converged: {}", analysis.healthy_converged);
    
    // Protocol should tolerate f < n/3 failures
    assert!(analysis.protocol_succeeded);
    assert!(analysis.failed_devices.len() <= 2);
}

#[aura_test]
async fn test_crash_recovery() {
    // Simulate clean crash and recovery
    let device = ctx.create_device().await?;
    
    // Take state snapshot
    let pre_crash_state = device.checkpoint().await?;
    
    // Simulate crash
    device.crash().await?;
    
    // Restart with recovery
    let recovered = device.recover_from_checkpoint(pre_crash_state).await?;
    
    // Verify recovery
    assert_eq!(recovered.state(), pre_crash_state);
    assert!(recovered.is_operational());
    
    // Continue protocol after recovery
    let result = continue_protocol(recovered).await?;
    assert!(result.succeeded());
}

#[aura_test]
async fn test_byzantine_behavior() {
    // Create Byzantine device with specific behaviors
    let byzantine = ctx.create_byzantine_device(
        ByzantineProfile::new()
            .equivocate(0.5)        // Send different messages to different peers
            .corrupt_state(0.3)     // Corrupt internal state
            .delay_messages(100..500) // Selective delays
            .ignore_protocol(0.2)   // Ignore protocol rules
    ).await?;
    
    // Run protocol with Byzantine participant
    let result = execute_protocol_with_byzantine(byzantine).await;
    
    // Verify Byzantine tolerance
    assert!(result.succeeded_despite_byzantine());
    assert!(result.byzantine_detected());
    assert_eq!(result.blamed_devices(), vec![byzantine.id()]);
}
```

## Advanced Simulation Examples

The following examples demonstrate modern simulation patterns with async effect interception:

### Byzantine Fault Tolerance Testing

```rust
use aura_testkit::{aura_test};
use aura_simulator::{AsyncSimulationEngine, ByzantineInterceptor};

#[aura_test]
async fn test_byzantine_resharing_detection() {
    // Create simulation with automatic setup
    let sim = AsyncSimulationEngine::new().await?;
    
    // Add honest participants
    let alice = sim.add_participant().await?;
    let carol = sim.add_participant().await?;
    
    // Add Byzantine participant with corruption behavior
    let byzantine_bob = sim.add_byzantine_participant(
        ByzantineInterceptor::new()
            .on_operation(Operation::ProduceResharingSubShare)
            .corrupt_messages(1.0)  // Always corrupt resharing messages
            .with_corruption_fn(|msg| {
                // Specific corruption for resharing
                let mut corrupted = msg.clone();
                corrupted.invalidate_proof();
                corrupted
            })
    ).await?;
    
    // Execute resharing protocol
    let action = Action::InitiateResharing {
        new_participants: vec![alice, byzantine_bob, carol],
        threshold: 2,
    };
    
    sim.tell_participant(alice, action).await?;
    
    // Run until completion with timeout
    sim.run_until_idle_with_timeout(Duration::from_secs(10)).await?;
    
    // Verify Byzantine behavior was detected
    let alice_state = sim.device_state(alice).await?;
    let last_event = alice_state.last_event();
    
    assert_matches!(
        last_event.payload,
        EventPayload::ResharingAborted { 
            reason: AbortReason::InvalidShare { from } 
        } if from == byzantine_bob
    );
    
    // Verify blame assignment
    assert!(alice_state.is_device_blamed(byzantine_bob));
    
    // Check simulation metrics
    let metrics = sim.metrics();
    assert_eq!(metrics.byzantine_detections, 1);
    assert!(metrics.protocol_completed_despite_byzantine);
}

### Sophisticated Byzantine Patterns

```rust
#[aura_test]
async fn test_complex_byzantine_scenarios() {
    let sim = AsyncSimulationEngine::new().await?;
    
    // Adaptive Byzantine that changes behavior
    let adaptive_byzantine = sim.add_byzantine_participant(
        AdaptiveByzantineInterceptor::new()
            .phase(ProtocolPhase::Setup, |interceptor| {
                interceptor.behave_honestly()  // Act honest initially
            })
            .phase(ProtocolPhase::Execution, |interceptor| {
                interceptor.corrupt_selectively(0.5)  // Corrupt 50% in execution
            })
            .phase(ProtocolPhase::Finalization, |interceptor| {
                interceptor.block_all_messages()  // Block during finalization
            })
    ).await?;
    
    // Colluding Byzantine devices
    let colluder1 = sim.add_byzantine_participant(
        ColludingByzantineInterceptor::new()
            .collude_with(colluder2_id)
            .share_private_state()
            .coordinate_attacks()
    ).await?;
    
    // Eclipse attack Byzantine
    let eclipse_attacker = sim.add_byzantine_participant(
        EclipseAttackInterceptor::new()
            .target_device(victim_id)
            .isolate_from_network()
            .feed_false_information()
    ).await?;
    
    // Execute protocol and verify resilience
    let result = execute_complex_protocol(&sim).await?;
    
    assert!(result.succeeded_despite_byzantines());
    assert_eq!(result.detected_byzantines.len(), 3);
}
```

### Network Partition and Recovery

```rust
#[aura_test]
async fn test_partition_tolerance_and_recovery() {
    let sim = AsyncSimulationEngine::new().await?;
    
    // Create 5-device threshold setup
    let devices = sim.add_participants(5).await?;
    
    // Start protocol
    let protocol_handle = sim.spawn_protocol(
        ThresholdSigningProtocol {
            message: b"test message",
            signers: devices.clone(),
            threshold: 3,
        }
    );
    
    // Let it progress
    advance_time_by(Duration::from_secs(2));
    
    // Create dynamic partition
    let partition_controller = sim.network()
        .create_partition_controller();
    
    // Partition 1: Split 3-2
    partition_controller.partition(
        devices[..3].to_vec(),
        devices[3..].to_vec(),
    ).await?;
    
    // Run for 5 seconds with partition
    advance_time_by(Duration::from_secs(5));
    
    // Heal partition partially (4-1 split)
    partition_controller.move_device(devices[3], PartitionGroup::A).await?;
    advance_time_by(Duration::from_secs(2));
    
    // Full heal
    partition_controller.heal_all().await?;
    
    // Protocol should complete
    let result = protocol_handle.await?;
    assert!(result.succeeded());
    
    // Verify eventual consistency
    let states = sim.all_device_states().await?;
    let reference = &states[0];
    for state in &states[1..] {
        assert_eq!(state.merkle_root(), reference.merkle_root());
    }
    
    // Check partition metrics
    let metrics = sim.network().partition_metrics();
    assert_eq!(metrics.total_partitions, 2);
    assert_eq!(metrics.healed_partitions, 2);
    assert!(metrics.messages_dropped_by_partition > 0);
}

#[aura_test]
async fn test_asymmetric_partitions() {
    // Test one-way communication failures
    let sim = AsyncSimulationEngine::new().await?;
    let [a, b, c] = sim.add_participants(3).await?[..] else { panic!() };
    
    // A can send to B, but B cannot send to A
    sim.network()
        .add_directional_partition(b, a)
        .allow_direction(a, b);
    
    // This creates interesting protocol challenges
    let result = execute_protocol(&[a, b, c]).await?;
    assert!(result.succeeded_with_degraded_performance());
}
```

### Complex Multi-Phase Protocols

```rust
#[aura_test]
async fn test_account_recovery_under_adverse_conditions() {
    let sim = AsyncSimulationEngine::new().await?;
    
    // Setup recovery scenario
    let alice_old = sim.add_participant().await?;
    let alice_new = sim.add_participant().await?;
    let guardians = sim.add_participants(3).await?;
    
    // Configure realistic WAN conditions
    sim.network()
        .add_latency(20..200)      // High latency
        .add_jitter(50)            // High jitter
        .add_packet_loss(0.02)     // 2% loss
        .add_reorder_rate(0.05);   // 5% reordering
    
    // Phase 1: Initiate recovery
    let recovery = RecoveryProtocol {
        old_device: alice_old,
        new_device: alice_new,
        guardians: guardians.clone(),
        threshold: 2,
    };
    
    let recovery_handle = sim.spawn_protocol(recovery);
    
    // Phase 2: Inject failures during recovery
    advance_time_by(Duration::from_secs(2));
    
    // Guardian 3 becomes slow/unreliable  
    sim.network()
        .throttle_device(guardians[2], 100) // 100ms extra latency
        .add_device_packet_loss(guardians[2], 0.3); // 30% packet loss
    
    // Phase 3: Guardian 1 temporarily partitioned
    advance_time_by(Duration::from_secs(3));
    sim.network().isolate_device(guardians[0]);
    
    // Phase 4: Heal partition after 5 seconds
    advance_time_by(Duration::from_secs(5));
    sim.network().reconnect_device(guardians[0]);
    
    // Recovery should complete despite failures
    let result = recovery_handle.await?;
    assert!(result.succeeded());
    
    // Verify new device has access
    let alice_new_state = sim.device_state(alice_new).await?;
    assert!(alice_new_state.has_device_access(alice_new));
    assert!(!alice_new_state.has_device_access(alice_old));
    
    // Verify eventual consistency
    let all_states = sim.all_device_states().await?;
    verify_merkle_root_convergence(&all_states)?;
    
    // Analyze recovery performance
    let recovery_analysis = sim.analyze_protocol_execution(&recovery_handle);
    println!("Recovery Metrics:");
    println!("  Total time: {:?}", recovery_analysis.total_duration);
    println!("  Messages exchanged: {}", recovery_analysis.message_count);
    println!("  Retransmissions: {}", recovery_analysis.retransmission_count);
    println!("  Guardian participation:");
    for (i, guardian) in guardians.iter().enumerate() {
        let participation = recovery_analysis.device_participation(guardian);
        println!("    Guardian {}: {:.1}%", i, participation * 100.0);
    }
    
    // Should complete despite 1 slow guardian
    assert!(recovery_analysis.total_duration < Duration::from_secs(30));
    assert!(recovery_analysis.successful_with_degradation);
}
```

## Advanced Simulation Techniques

### State Space Exploration

The simulation engine supports systematic exploration of protocol state spaces:

```rust
#[aura_test]
async fn test_exhaustive_state_exploration() {
    let explorer = StateSpaceExplorer::new();
    
    // Define protocol to explore
    let protocol = ThresholdProtocol {
        participants: 3,
        threshold: 2,
    };
    
    // Configure exploration parameters
    explorer.configure()
        .max_depth(10)
        .enable_symmetry_reduction()
        .enable_partial_order_reduction()
        .with_state_limit(10_000);
    
    // Explore all possible executions
    let exploration = explorer.explore(protocol).await?;
    
    println!("States explored: {}", exploration.total_states);
    println!("Unique states: {}", exploration.unique_states);
    println!("Terminal states: {}", exploration.terminal_states);
    
    // Check safety properties
    for invariant in &protocol.safety_invariants() {
        assert!(
            exploration.all_states_satisfy(invariant),
            "Invariant violated: {}", invariant.name()
        );
    }
    
    // Check liveness properties  
    for property in &protocol.liveness_properties() {
        assert!(
            exploration.eventually_satisfied(property),
            "Liveness violated: {}", property.name()
        );
    }
}

### Property-Based Protocol Testing

Combine simulation with property testing for thorough validation:

```rust
use proptest::prelude::*;

proptest! {
    #[aura_test]
    async fn protocol_maintains_invariants(
        device_count in 3..10usize,
        failure_rate in 0.0..0.3f64,
        network_latency in 1..100u64,
        message_loss in 0.0..0.1f64,
    ) {
        let sim = AsyncSimulationEngine::new().await?;
        
        // Create devices
        let devices = sim.add_participants(device_count).await?;
        
        // Configure network from generated parameters
        sim.network()
            .add_latency(network_latency..network_latency*2)
            .add_packet_loss(message_loss);
        
        // Add random failures
        let num_failures = ((device_count as f64) * failure_rate) as usize;
        for i in 0..num_failures {
            sim.fail_device_at_random_time(devices[i]).await?;
        }
        
        // Run protocol
        let result = execute_protocol(&sim, &devices).await;
        
        // Check invariants regardless of outcome
        let invariant_checker = InvariantChecker::new(&sim);
        
        // Safety: No double spending
        prop_assert!(invariant_checker.check_no_double_spend().await?);
        
        // Consistency: All honest devices agree
        prop_assert!(invariant_checker.check_honest_agreement().await?);
        
        // Liveness: Protocol completes if enough devices online
        let online_count = device_count - num_failures;
        if online_count >= (device_count * 2 / 3) {
            prop_assert!(result.is_ok());
        }
    }
}
```

### Simulation Analysis and Debugging

The simulation provides rich analysis capabilities:

```rust
#[aura_test]
async fn analyze_protocol_execution() {
    let sim = AsyncSimulationEngine::new()
        .with_detailed_tracing()
        .with_performance_profiling()
        .build()
        .await?;
    
    // Run protocol
    let devices = sim.add_participants(5).await?;
    execute_complex_protocol(&sim, &devices).await?;
    
    // Get comprehensive trace
    let trace = sim.get_trace().await?;
    let analyzer = TraceAnalyzer::new(trace);
    
    // Analyze critical paths
    let critical_path = analyzer.find_critical_path();
    println!("Critical path length: {:?}", critical_path.duration);
    println!("Critical path operations:");
    for (i, op) in critical_path.operations.iter().enumerate() {
        println!("  {}: {} ({:?})", i, op.name, op.duration);
    }
    
    // Find communication hotspots
    let hotspots = analyzer.find_communication_hotspots();
    for hotspot in &hotspots {
        println!("Hotspot: {} ↔ {} ({} messages)", 
            hotspot.device_a, 
            hotspot.device_b, 
            hotspot.message_count
        );
    }
    
    // Detect anomalies
    let anomalies = analyzer.detect_anomalies();
    for anomaly in &anomalies {
        println!("Anomaly: {} at {:?}", anomaly.description, anomaly.timestamp);
        println!("  Affected devices: {:?}", anomaly.devices);
        println!("  Suggested investigation: {}", anomaly.suggestion);
    }
    
    // Generate visualization
    let viz = analyzer.generate_visualization();
    viz.save_sequence_diagram("protocol_execution.svg")?;
    viz.save_state_timeline("state_evolution.svg")?;
    viz.save_message_heatmap("communication_pattern.svg")?;
    
    // Performance analysis
    let perf = sim.performance_report();
    println!("\nPerformance Summary:");
    println!("  Total duration: {:?}", perf.total_duration);
    println!("  Messages/second: {:.0}", perf.message_throughput);
    println!("  State operations/second: {:.0}", perf.state_throughput);
    println!("  Effect processing overhead: {:.1}%", perf.effect_overhead * 100.0);
    println!("  Simulation slowdown: {:.2}x", perf.slowdown_factor);
}

## Summary

Aura's async simulation infrastructure provides:

- **Production Fidelity** - Test actual protocol implementations with real async effect system
- **WASM Compatibility** - All simulation features work in browser environments
- **Rich Fault Modeling** - Byzantine behaviors, network conditions, and failure patterns
- **Deterministic Execution** - Perfect reproducibility with time control and seeded randomness
- **Comprehensive Analysis** - Detailed tracing, performance profiling, and visualization
- **Property Testing** - Systematic validation of safety and liveness properties

The simulation integrates seamlessly with the `#[aura_test]` macro and async effect system, enabling thorough validation of distributed protocols under all possible conditions.

### Best Practices

1. **Start Simple** - Use basic network simulation before adding Byzantine behaviors
2. **Incremental Complexity** - Add failures and partitions gradually
3. **Property Focus** - Define clear invariants and check them systematically
4. **Performance Awareness** - Monitor simulation overhead and optimize hot paths
5. **Reproducibility First** - Always use deterministic configurations for debugging

For testing infrastructure that complements simulation see [Testing Guide](805_testing_guide.md). Learn about the async effect system in [System Architecture](002_system_architecture.md). Review the refactor progress in [Async Test Plan](../work/async_test.md).