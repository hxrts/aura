# Advanced Choreography Guide

This guide covers sophisticated choreographic programming patterns using rumpsteak-aura. You will learn comprehensive DSL syntax, projection patterns, effect handlers, extension systems, and composition techniques for building complex distributed protocols.

## Comprehensive DSL Syntax

Rumpsteak-aura provides a rich choreographic DSL for specifying distributed protocols. The parser supports protocol definitions, role declarations, message passing, choice constructs, loops, parallel composition, and comprehensive annotation systems.

### Protocol Namespacing

Aura supports protocol namespacing through the `choreography!` macro for module organization and conflict prevention:

```rust
use aura_macros::choreography;

choreography! {
    #[namespace = "admin_operations"]
    protocol AdminOperations {
        roles: Admin, Moderator, User;
        
        Admin -> Moderator: GrantPermissions;
        Moderator -> User: ModerationAction;
    }
}

choreography! {
    #[namespace = "user_management"]
    protocol UserRegistration {
        roles: NewUser, Validator, Admin;
        
        NewUser -> Validator: RegistrationRequest;
        Validator -> Admin: ValidationResult;
    }
}
```

Namespaces generate separate module structures in code generation. Protocol names remain isolated within their namespace scope. Multiple choreographies can define similar role or message names without conflicts.

### Message Types and Type Annotations

Rumpsteak-aura supports rich message type specifications with optional type annotations:

```rust
let protocol_with_types = r#"
    choreography TypedMessages {
        roles: Client, Server;
        
        Client -> Server: Request<String>;
        Server -> Client: Response<i32>;
        Client -> Server: Data<Vec<String>, HashMap<String, i32>>;
    }
"#;

let advanced_types = r#"
    choreography AdvancedTypes {
        roles: A, B;
        
        A -> B: Container<std::vec::Vec<String>>;
        B -> A: Result<i32, CustomError>;
        A -> B: Request<String>(payload_data);
    }
"#;
```

Message type annotations provide strong typing for protocol data. Generic types and nested generics are fully supported. Path types enable fully qualified type specifications.

### Dynamic Role Count Support

Rumpsteak-aura provides comprehensive support for protocols with variable participant counts determined at runtime:

#### Runtime-Determined Participants

```rust
let threshold_protocol = r#"
    choreography ThresholdProtocol {
        roles: Coordinator, Signers[*];
        
        Coordinator -> Signers[*]: Request;
        Signers[0..threshold] -> Coordinator: Response;
    }
"#;

let consensus_protocol = r#"
    choreography ConsensusProtocol {
        roles: Leader, Followers[N];
        
        Leader -> Followers[*]: Proposal;
        Followers[i] -> Leader: Vote;
    }
"#;
```

Wildcard syntax `[*]` indicates runtime-determined counts. Symbolic parameters `[N]` provide compile-time flexibility. Range expressions `[0..threshold]` target subsets of participants.

#### Fixed-Count Dynamic Roles

```rust
let static_roles = r#"
    choreography StaticWorkers {
        roles: Master, Worker[3];
        
        Master -> Worker[0]: Task1;
        Master -> Worker[1]: Task2;
        Worker[0] -> Master: Result1;
    }
"#;

```

Static role arrays define exactly three worker instances. Individual workers are addressed using index notation. The protocol specifies explicit interactions with each worker instance.

#### Runtime Role Binding

```rust
use rumpsteak_aura_choreography::compiler::codegen::generate_choreography_code_with_dynamic_roles;

let consensus_protocol = r#"
    choreography ConsensusProtocol {
        roles: Leader, Followers[N];
        
        Leader -> Followers[*]: Proposal;
        Followers[i] -> Leader: Vote;
    }
"#;

let choreo = parse_choreography_str(consensus_protocol)?;
let code = generate_choreography_code_with_dynamic_roles(&choreo, &local_types);

// Runtime binding
let mut runtime = ConsensusRuntime::new();
runtime.bind_role_count("Followers", 5)?;
runtime.map_followers_instances(vec!["alice", "bob", "charlie", "dave", "eve"])?;
```

Symbolic parameters `[N]` enable compile-time role count flexibility. Runtime binding maps symbolic counts to concrete participant numbers. Instance mapping assigns specific identities to role positions.

### Choice Constructs and Branching

Rumpsteak-aura supports rich choice constructs with guards and multi-way branching:

#### Basic Choice

```rust
let negotiation_protocol = r#"
    choreography Negotiation {
        roles: Buyer, Seller;
        
        Buyer -> Seller: Offer;
        
        choice Seller {
            accept: {
                Seller -> Buyer: Accept;
            }
            reject: {
                Seller -> Buyer: Reject;
            }
        }
    }
"#;
```

Choice constructs enable protocol branching based on role decisions. Each branch defines continuation behavior. Label matching ensures type-safe branch selection.

#### Guarded Choice

```rust
let conditional_protocol = r#"
    choreography ConditionalProtocol {
        roles: Client, Server;
        
        choice Client {
            buy when (balance > price): {
                Client -> Server: Purchase;
            }
            cancel: {
                Client -> Server: Cancel;
            }
        }
    }
"#;
```

Guarded choices add conditional logic to branch selection. Guard expressions use valid Rust boolean syntax. Guards enable protocol behavior based on runtime conditions.

### Loop Constructs

Rumpsteak-aura supports various loop patterns for iterative protocols:

#### Fixed Iteration Count

```rust
let loop_protocol = r#"
    choreography LoopProtocol {
        roles: A, B;
        
        loop (count: 5) {
            A -> B: Request;
            B -> A: Response;
        }
    }
"#;
```

Fixed count loops execute the body a specified number of times. Loop bodies can contain arbitrary protocol operations. Nested loops are supported for complex iteration patterns.

#### Role-Controlled Loops

```rust
let decision_loop = r#"
    choreography DecisionLoop {
        roles: Client, Server;
        
        loop (decides: Client) {
            Client -> Server: Request;
            Server -> Client: Response;
        }
    }
"#;
```

Role-controlled loops allow participants to decide when to continue or exit. The deciding role controls loop termination. This enables adaptive protocol behavior based on runtime conditions.

## Aura-Specific Extensions

Aura extends rumpsteak-aura with domain-specific annotations for security, performance, and state management. These extensions integrate with aura-mpst for runtime enforcement and verification.

### Guard Capabilities

Guard capabilities provide authorization control for protocol operations:

```rust
use aura_macros::choreography;

choreography! {
    #[namespace = "secure_protocol"]
    protocol SecureProtocol {
        roles: Client, Server;
        
        Client[guard_capability = "send_request"]
        -> Server: Request(RequestData);
        
        Server[guard_capability = "process_request"]
        -> Client: Response(ResponseData);
    }
}
```

Guard capabilities specify required permissions for protocol operations. The `guard_capability` annotation compiles to `CapabilityGuardEffect` instances that integrate with rumpsteak-aura's extension system. The `AuraHandler` validates capabilities through registered extension handlers before executing protocol steps. Failed capability checks prevent unauthorized operations and return appropriate error responses.

### Flow Cost Control

Flow costs control privacy budget usage and rate limiting:

```rust
choreography! {
    #[namespace = "budgeted_protocol"]
    protocol BudgetedProtocol {
        roles: Sender, Receiver;
        
        Sender[flow_cost = 200]
        -> Receiver: HighCostMessage(LargeData);
        
        Receiver[flow_cost = 50]
        -> Sender: LowCostAck(AckData);
        
        // Messages with annotations but no flow_cost get default of 100
        Sender[guard_capability = "send_data"]
        -> Receiver: DefaultCostMessage(Data);
    }
}
```

Flow costs specify budget consumption per protocol operation. Higher costs indicate more expensive operations in terms of privacy or resources. Budget enforcement prevents excessive resource usage.

When a message has role annotations (brackets with any Aura-specific attributes) but no explicit `flow_cost` specified, a default value of 100 is automatically applied. This ensures all annotated protocol operations have flow budget tracking without requiring explicit cost specification for every message.

### Journal Facts

Journal facts enable distributed state tracking and auditability:

```rust
choreography! {
    #[namespace = "auditable_protocol"]
    protocol AuditableProtocol {
        roles: Coordinator, Participant;
        
        Coordinator[journal_facts = "operation_initiated"]
        -> Participant: InitiateOperation(OperationData);
        
        Participant[journal_facts = "operation_completed"]
        -> Coordinator: OperationResult(ResultData);
    }
}
```

Journal facts record protocol events in distributed state. Facts enable protocol auditing and state reconstruction. Multiple facts use comma separation for complex state tracking.

### Combined Annotations

Annotations combine for comprehensive protocol control:

```rust
choreography! {
    #[namespace = "comprehensive_protocol"]
    protocol ComprehensiveProtocol {
        roles: Admin, User;
        
        Admin[guard_capability = "admin_access",
              flow_cost = 200,
              journal_facts = "admin_action_logged"]
        -> User: AdminCommand(CommandData);
        
        User[guard_capability = "respond_to_admin",
             flow_cost = 100,
             journal_facts = "user_response_recorded"]
        -> Admin: UserResponse(ResponseData);
    }
}
```

Combined annotations compile to multiple extension effects that execute in sequence during protocol operations. Guard capabilities ensure authorization through `CapabilityGuardEffect`. Flow costs control resource usage through `FlowCostEffect`. Journal facts enable auditability through `JournalFactsEffect`. All effects are registered in the `ExtensionRegistry` and execute automatically during choreographic message operations.

## Effect System Integration

Rumpsteak-aura protocols integrate with algebraic effect systems for runtime execution. Effect programs provide composable protocol building blocks for complex distributed systems.

### Effect Programs

Aura choreographies generate session types that integrate with rumpsteak-aura's effect system:

```rust
use aura_protocol::choreography::AuraHandlerAdapter;

let mut handler = AuraHandlerAdapter::for_testing(device_id)?;
let mut endpoint = AuraEndpoint::new(context_id);

// Execute choreography-generated protocol
let result = execute_alice_role(&mut handler, &mut endpoint).await?;
```

Generated protocols execute through rumpsteak-aura's `interpret_extensible` function using the `AuraHandler`. Extension effects execute automatically for annotated messages, providing capability guards, flow budgets, and journal coupling. Results contain received values and execution state with Aura-specific metadata from extension effect execution.

### Handler Abstraction

Aura provides specialized handlers that implement rumpsteak-aura's `ChoreoHandler` trait:

```rust
use aura_protocol::choreography::AuraHandlerAdapter;

// Testing handler with in-memory effects
let test_handler = AuraHandlerAdapter::for_testing(device_id)?;

// Production handler with real network and storage
let prod_handler = AuraHandlerAdapter::for_production(device_id)?;

// Simulation handler with fault injection
let sim_handler = AuraHandlerAdapter::for_simulation(device_id, config)?;
```

Different handler modes provide testing capabilities, production deployment, and simulation environments. Each handler implements both `ChoreoHandler` and `ExtensibleHandler` traits with appropriate extension registries for that execution mode. The `AuraHandler` integrates with Aura's effect system through registered extension effects that execute during protocol operations. Handler selection enables flexible protocol execution environments while maintaining consistent choreographic interfaces.

### Protocol Composition

Effect programs enable protocol composition and reuse:

```rust
let handshake = Program::new()
    .send(Role::Bob, Message("hello"))
    .recv::<Message>(Role::Bob)
    .end();

let data_transfer = Program::new()
    .send(Role::Bob, Data(vec![1, 2, 3]))
    .recv::<Ack>(Role::Bob)
    .end();

let composed_protocol = Program::new()
    .then(handshake)
    .then(data_transfer)
    .end();
```

Protocol composition uses the `then` method for sequential execution. Complex protocols build from simpler components. Composition maintains type safety and session properties.

## Integration with Aura Systems

Choreographic protocols integrate with broader Aura infrastructure through standardized interfaces. Protocol execution uses effect handlers from aura-effects and coordination primitives from aura-protocol. Guard capabilities integrate with the web of trust system.

Choreographies compose with CRDT programming patterns for state consistency. Flow budget enforcement prevents excessive resource usage. Journal facts enable distributed auditability across protocol execution.

For foundational concepts, see [Coordination Systems Guide](803_coordination_systems_guide.md). For testing approaches, see [Simulation and Testing Guide](805_simulation_and_testing_guide.md).

Ring protocols ensure ordered processing across participants. Token passing accumulates state changes. Each node contributes to the final decision. Journal merging synchronizes accumulated state.

### Hierarchical Coordination

Structure protocols with coordinator hierarchies:

```rust
choreography! {
    #[namespace = "hierarchical_consensus"]
    protocol HierarchicalConsensus {
        roles: TopCoordinator, SubCoordinator1, SubCoordinator2, 
               Worker1, Worker2, Worker3, Worker4;
        
        // Top-level coordination
        TopCoordinator[guard_capability = "coordinate_top_level",
                       flow_cost = 300,
                       journal_facts = "top_coordination_started"]
        -> SubCoordinator1: CoordinationRequest(TopLevelRequest);
        
        TopCoordinator[guard_capability = "coordinate_top_level",
                       flow_cost = 300]
        -> SubCoordinator2: CoordinationRequest(TopLevelRequest);
        
        // Sub-level worker coordination
        SubCoordinator1[guard_capability = "coordinate_workers",
                        flow_cost = 150,
                        journal_facts = "worker_coordination_started"]
        -> Worker1: WorkerRequest(WorkerTask);
        
        SubCoordinator1[guard_capability = "coordinate_workers",
                        flow_cost = 150]
        -> Worker2: WorkerRequest(WorkerTask);
        
        SubCoordinator2[guard_capability = "coordinate_workers",
                        flow_cost = 150]
        -> Worker3: WorkerRequest(WorkerTask);
        
        SubCoordinator2[guard_capability = "coordinate_workers",
                        flow_cost = 150]
        -> Worker4: WorkerRequest(WorkerTask);
        
        // Result aggregation up the hierarchy
        Worker1[guard_capability = "submit_result",
                flow_cost = 80,
                journal_facts = "work_completed"]
        -> SubCoordinator1: WorkerResult(ResultData);
        
        SubCoordinator1[guard_capability = "aggregate_results",
                        flow_cost = 200,
                        journal_facts = "sub_results_aggregated",
                        journal_merge = true]
        -> TopCoordinator: SubCoordinatorResult(AggregatedResult);
    }
}
```

Hierarchical protocols scale to large participant counts. Sub-coordinators manage worker groups. Results aggregate up through coordinator levels. This pattern enables efficient resource utilization.

## Protocol Composition and Layering

Complex applications require composing multiple protocols and layering different coordination mechanisms. Composition patterns enable building sophisticated distributed systems.

### Sequential Protocol Composition

Chain protocols to create multi-phase workflows:

```rust
pub struct SequentialProtocolRunner<E: Effects> {
    effects: E,
    device_id: aura_core::DeviceId,
}

impl<E: Effects> SequentialProtocolRunner<E> {
    pub async fn execute_authentication_flow(
        &self,
        target_device: aura_core::DeviceId,
    ) -> Result<AuthenticationResult, ProtocolError> {
        // Phase 1: Identity exchange
        let identity_result = self.execute_identity_exchange(target_device).await?;
        
        // Phase 2: Capability negotiation
        let capability_result = self.execute_capability_negotiation(
            target_device,
            &identity_result
        ).await?;
        
        // Phase 3: Session establishment
        let session_result = self.execute_session_establishment(
            target_device,
            &capability_result
        ).await?;
        
        Ok(AuthenticationResult {
            identity: identity_result,
            capabilities: capability_result,
            session: session_result,
        })
    }
}
```

Sequential composition executes protocols in dependency order. Each phase uses results from previous phases. Failed phases abort the entire workflow. State accumulates across phase boundaries.

### Parallel Protocol Execution

Execute multiple protocols concurrently with synchronization points:

```rust
pub struct ParallelProtocolCoordinator<E: Effects> {
    effects: E,
    coordination_config: CoordinationConfig,
}

impl<E: Effects> ParallelProtocolCoordinator<E> {
    pub async fn execute_distributed_computation(
        &self,
        computation_request: ComputationRequest,
        worker_devices: Vec<aura_core::DeviceId>,
    ) -> Result<ComputationResult, ProtocolError> {
        // Launch parallel worker protocols
        let worker_futures = worker_devices.into_iter().map(|device| {
            self.execute_worker_protocol(device, computation_request.clone())
        });
        
        // Wait for all workers with timeout
        let worker_results = tokio::time::timeout(
            self.coordination_config.worker_timeout,
            futures::future::try_join_all(worker_futures)
        ).await??;
        
        // Aggregate results from all workers
        let aggregated_result = self.aggregate_worker_results(worker_results).await?;
        
        // Broadcast final result to all workers
        let broadcast_futures = worker_devices.into_iter().map(|device| {
            self.broadcast_final_result(device, &aggregated_result)
        });
        
        futures::future::try_join_all(broadcast_futures).await?;
        
        Ok(aggregated_result)
    }
}
```

Parallel execution maximizes resource utilization. Synchronization points coordinate distributed phases. Timeout handling manages unresponsive participants. Result aggregation combines parallel outputs.

### Protocol Adaptation Layers

Adapt between different protocol interfaces:

```rust
pub struct ProtocolAdapter<E: Effects> {
    effects: E,
    adaptation_config: AdaptationConfig,
}

impl<E: Effects> ProtocolAdapter<E> {
    pub async fn adapt_legacy_to_modern(
        &self,
        legacy_request: LegacyProtocolRequest,
    ) -> Result<ModernProtocolResult, AdaptationError> {
        // Convert legacy request format
        let modern_request = self.convert_request_format(legacy_request)?;
        
        // Execute modern protocol
        let modern_result = self.execute_modern_protocol(modern_request).await?;
        
        // Convert result back to legacy format if needed
        let adapted_result = self.convert_result_format(modern_result)?;
        
        Ok(adapted_result)
    }
    
    async fn execute_capability_bridging(
        &self,
        source_capabilities: CapabilitySet,
        target_protocol: ProtocolType,
    ) -> Result<BridgedCapabilities, AdaptationError> {
        // Map capabilities between different systems
        let mapped_capabilities = self.map_capability_semantics(
            source_capabilities,
            target_protocol
        )?;
        
        // Verify capability compatibility
        self.verify_capability_compatibility(&mapped_capabilities, target_protocol).await?;
        
        Ok(BridgedCapabilities {
            original: source_capabilities,
            mapped: mapped_capabilities,
            protocol: target_protocol,
        })
    }
}
```

Adaptation layers bridge different protocol generations. Format conversion handles message compatibility. Capability mapping preserves security properties across protocol boundaries.

## Error Handling and Resilience

Production choreographic protocols require comprehensive error handling and resilience mechanisms. These patterns ensure graceful degradation under adverse conditions.

### Timeout and Retry Patterns

Implement robust timeout handling with exponential backoff:

```rust
pub struct ResilientProtocolExecutor<E: Effects> {
    effects: E,
    retry_config: RetryConfig,
}

impl<E: Effects> ResilientProtocolExecutor<E> {
    pub async fn execute_with_resilience<T>(
        &self,
        protocol_fn: impl Fn() -> Pin<Box<dyn Future<Output = Result<T, ProtocolError>> + Send>>,
        operation_name: &str,
    ) -> Result<T, ProtocolError> {
        let mut attempt = 0;
        let mut last_error = None;
        
        while attempt < self.retry_config.max_attempts {
            match tokio::time::timeout(
                self.retry_config.operation_timeout,
                protocol_fn()
            ).await {
                Ok(Ok(result)) => {
                    if attempt > 0 {
                        self.effects.log_info(&format!(
                            "Protocol {} succeeded on attempt {}",
                            operation_name, attempt + 1
                        )).await;
                    }
                    return Ok(result);
                }
                Ok(Err(e)) => {
                    last_error = Some(e.clone());
                    
                    if !e.is_retryable() || attempt >= self.retry_config.max_attempts - 1 {
                        break;
                    }
                    
                    self.effects.log_warn(&format!(
                        "Protocol {} attempt {} failed: {}, retrying...",
                        operation_name, attempt + 1, e
                    )).await;
                }
                Err(_) => {
                    let timeout_error = ProtocolError::Timeout {
                        operation: operation_name.to_string(),
                        timeout: self.retry_config.operation_timeout,
                    };
                    last_error = Some(timeout_error);
                    
                    if attempt >= self.retry_config.max_attempts - 1 {
                        break;
                    }
                }
            }
            
            // Exponential backoff with jitter
            let delay = self.retry_config.base_delay * 2_u32.pow(attempt);
            let jittered_delay = self.add_jitter(delay);
            tokio::time::sleep(jittered_delay).await;
            
            attempt += 1;
        }
        
        Err(last_error.unwrap_or_else(|| ProtocolError::UnknownFailure))
    }
    
    fn add_jitter(&self, delay: Duration) -> Duration {
        let jitter_range = delay / 4; // 25% jitter
        let jitter_ms = fastrand::u64(0..jitter_range.as_millis() as u64);
        delay + Duration::from_millis(jitter_ms)
    }
}
```

Resilient execution handles transient failures automatically. Exponential backoff prevents overwhelming failed services. Jitter reduces thundering herd effects. Retry limits prevent infinite loops.

### Compensation and Rollback

Implement compensation logic for failed multi-phase protocols:

```rust
pub struct CompensatingProtocolManager<E: Effects> {
    effects: E,
    compensation_log: CompensationLog,
}

impl<E: Effects> CompensatingProtocolManager<E> {
    pub async fn execute_compensating_transaction(
        &self,
        transaction: CompensatingTransaction,
    ) -> Result<TransactionResult, TransactionError> {
        let mut completed_operations = Vec::new();
        
        // Execute forward operations
        for operation in &transaction.operations {
            match self.execute_operation(operation).await {
                Ok(result) => {
                    completed_operations.push((operation.clone(), result));
                    self.compensation_log.record_operation(operation).await?;
                }
                Err(e) => {
                    // Compensation required
                    self.effects.log_error(&format!(
                        "Operation {} failed: {}, starting compensation",
                        operation.operation_id, e
                    )).await;
                    
                    self.execute_compensation(&completed_operations).await?;
                    
                    return Err(TransactionError::OperationFailed {
                        failed_operation: operation.clone(),
                        cause: e,
                        compensated: true,
                    });
                }
            }
        }
        
        // All operations succeeded
        self.compensation_log.mark_transaction_complete(&transaction.transaction_id).await?;
        
        Ok(TransactionResult {
            transaction_id: transaction.transaction_id,
            completed_operations,
            status: TransactionStatus::Committed,
        })
    }
    
    async fn execute_compensation(
        &self,
        completed_operations: &[(Operation, OperationResult)],
    ) -> Result<(), CompensationError> {
        // Execute compensations in reverse order
        for (operation, _result) in completed_operations.iter().rev() {
            match self.execute_compensation_for_operation(operation).await {
                Ok(_) => {
                    self.compensation_log.record_compensation(operation).await?;
                }
                Err(e) => {
                    self.effects.log_error(&format!(
                        "Compensation failed for operation {}: {}",
                        operation.operation_id, e
                    )).await;
                    
                    // Log compensation failure but continue with other compensations
                    self.compensation_log.record_compensation_failure(operation, e).await?;
                }
            }
        }
        
        Ok(())
    }
}
```

Compensation protocols ensure consistency despite partial failures. Operations execute in forward order. Compensations execute in reverse order. Compensation logging provides audit trails.

### Fault Isolation and Circuit Breakers

Implement circuit breaker patterns for fault isolation:

```rust
pub struct CircuitBreakerProtocolWrapper<E: Effects> {
    effects: E,
    circuit_state: Arc<Mutex<CircuitState>>,
    config: CircuitBreakerConfig,
}

#[derive(Clone)]
pub enum CircuitState {
    Closed { failure_count: usize },
    Open { opened_at: Instant },
    HalfOpen { test_requests: usize },
}

impl<E: Effects> CircuitBreakerProtocolWrapper<E> {
    pub async fn execute_with_circuit_breaker<T>(
        &self,
        protocol_fn: impl Fn() -> Pin<Box<dyn Future<Output = Result<T, ProtocolError>> + Send>>,
        service_name: &str,
    ) -> Result<T, ProtocolError> {
        // Check circuit state
        let should_execute = {
            let mut state = self.circuit_state.lock().unwrap();
            match &*state {
                CircuitState::Closed { failure_count } => {
                    *failure_count < self.config.failure_threshold
                }
                CircuitState::Open { opened_at } => {
                    let elapsed = opened_at.elapsed();
                    if elapsed >= self.config.recovery_timeout {
                        *state = CircuitState::HalfOpen { test_requests: 0 };
                        true
                    } else {
                        false
                    }
                }
                CircuitState::HalfOpen { test_requests } => {
                    *test_requests < self.config.test_request_threshold
                }
            }
        };
        
        if !should_execute {
            return Err(ProtocolError::CircuitBreakerOpen {
                service: service_name.to_string(),
            });
        }
        
        // Execute protocol
        match protocol_fn().await {
            Ok(result) => {
                self.record_success().await;
                Ok(result)
            }
            Err(e) => {
                self.record_failure(&e).await;
                Err(e)
            }
        }
    }
    
    async fn record_success(&self) {
        let mut state = self.circuit_state.lock().unwrap();
        *state = CircuitState::Closed { failure_count: 0 };
        
        self.effects.log_debug("Circuit breaker reset to closed state").await;
    }
    
    async fn record_failure(&self, error: &ProtocolError) {
        let mut state = self.circuit_state.lock().unwrap();
        
        match &*state {
            CircuitState::Closed { failure_count } => {
                let new_count = failure_count + 1;
                if new_count >= self.config.failure_threshold {
                    *state = CircuitState::Open { opened_at: Instant::now() };
                    
                    self.effects.log_warn(&format!(
                        "Circuit breaker opened due to {} failures",
                        new_count
                    )).await;
                } else {
                    *state = CircuitState::Closed { failure_count: new_count };
                }
            }
            CircuitState::HalfOpen { test_requests } => {
                *state = CircuitState::Open { opened_at: Instant::now() };
                
                self.effects.log_warn("Circuit breaker reopened during half-open test").await;
            }
            CircuitState::Open { .. } => {
                // Already open, no state change needed
            }
        }
    }
}
```

Circuit breakers prevent cascading failures across distributed systems. Failure thresholds trigger circuit opening. Recovery timeouts enable gradual service restoration. Half-open states test service health carefully.

## System Layering Techniques

Advanced choreographic applications layer multiple coordination mechanisms. Layering enables separation of concerns and modular system design.

### Capability Layer Integration

Layer capability checking across protocol boundaries:

```rust
pub struct LayeredCapabilityManager<E: Effects> {
    effects: E,
    capability_layers: Vec<CapabilityLayer>,
}

impl<E: Effects> LayeredCapabilityManager<E> {
    pub async fn evaluate_layered_capabilities(
        &self,
        request: LayeredCapabilityRequest,
    ) -> Result<LayeredCapabilityGrant, CapabilityError> {
        let mut capability_context = request.initial_context;
        let mut layer_grants = Vec::new();
        
        for (layer_index, layer) in self.capability_layers.iter().enumerate() {
            let layer_request = CapabilityRequest {
                requesting_device: request.requesting_device,
                operation: request.operation.clone(),
                context: capability_context.clone(),
                layer_index,
            };
            
            match layer.evaluate_capabilities(&layer_request).await {
                Ok(grant) => {
                    capability_context = grant.refined_context.clone();
                    layer_grants.push(grant);
                }
                Err(e) => {
                    return Err(CapabilityError::LayerDenied {
                        layer_index,
                        layer_name: layer.name().to_string(),
                        denial_reason: e.to_string(),
                    });
                }
            }
        }
        
        Ok(LayeredCapabilityGrant {
            final_context: capability_context,
            layer_grants,
            overall_authorization: AuthorizationLevel::Granted,
        })
    }
}
```

Capability layers refine authorization progressively. Each layer applies additional constraints. Layer failures deny the entire request. Context refinement accumulates across layers.

### Privacy Budget Layering

Layer privacy budget enforcement across protocol hierarchies:

```rust
pub struct LayeredPrivacyManager<E: Effects> {
    effects: E,
    privacy_layers: BTreeMap<PrivacyLayer, PrivacyBudgetManager>,
}

impl<E: Effects> LayeredPrivacyManager<E> {
    pub async fn charge_layered_privacy_cost(
        &self,
        operation: &PrivacyOperation,
        context: &PrivacyContext,
    ) -> Result<LayeredPrivacyReceipt, PrivacyError> {
        let mut layer_receipts = Vec::new();
        let base_cost = operation.calculate_base_cost();
        
        for (layer, budget_manager) in &self.privacy_layers {
            let layer_cost = layer.calculate_layer_cost(&base_cost, context);
            
            match budget_manager.charge_budget(context, layer_cost).await {
                Ok(receipt) => {
                    layer_receipts.push(LayerReceipt {
                        layer: layer.clone(),
                        cost_charged: layer_cost,
                        receipt,
                    });
                }
                Err(e) => {
                    // Rollback previous charges
                    self.rollback_privacy_charges(&layer_receipts).await?;
                    
                    return Err(PrivacyError::BudgetExhausted {
                        layer: layer.clone(),
                        required_cost: layer_cost,
                        cause: e,
                    });
                }
            }
        }
        
        Ok(LayeredPrivacyReceipt {
            operation_id: operation.operation_id,
            total_cost: base_cost,
            layer_receipts,
            timestamp: self.effects.current_timestamp().await,
        })
    }
}
```

Privacy budget layering enables fine-grained leakage control. Different layers track different privacy concerns. Budget exhaustion at any layer denies operations. Rollback mechanisms ensure consistency.

Advanced choreographic programming enables building sophisticated distributed systems with strong security and privacy guarantees. These patterns provide the foundation for complex real-world applications.

Continue with [Simulation and Testing Guide](805_simulation_and_testing_guide.md) for comprehensive protocol testing approaches using Aura's simulation infrastructure.
