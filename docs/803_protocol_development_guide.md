# Protocol Development Guide

> **Important Note**: This guide shows the **intended** choreographic programming approach for Aura. Currently, most protocols are implemented manually using the effect system while choreographic DSL integration is in development. For the actual rumpsteak-aura DSL syntax, see [Choreography Programming Guide](805_choreography_programming_guide.md).

Aura's choreographic programming enables writing distributed protocols from a global perspective. The choreography compiler generates local implementations for each participant with session type safety guarantees.

This guide covers choreographic programming patterns, protocol composition techniques, error handling strategies, and testing approaches for distributed systems. You will learn to build reliable coordination protocols.

See [Getting Started Guide](800_getting_started_guide.md) for basic concepts. For effect system infrastructure integration, see [System Architecture](002_system_architecture.md#1-unified-effect-system-architecture).

---

## Choreographic Programming

**Global Perspective** describes protocols from the viewpoint of an omniscient observer. For complete choreography system documentation including DSL syntax, projection rules, and execution patterns, see [Choreography System Reference](302_choreography_system.md).

```rust
// Note: This shows the ideal choreographic syntax for documentation purposes.
// Current implementation requires manual protocol implementations using the effect system.
// See docs/805_choreography_programming_guide.md for actual rumpsteak-aura DSL syntax.

// Actual rumpsteak-aura syntax (when using DSL):
choreography TwoPhaseCommit {
    roles: Coordinator, Participant

    Coordinator -> Participant: PrepareRequest
    Participant -> Coordinator: PrepareResponse

    choice Participant {
        commit: {
            Coordinator -> Participant: CommitRequest
            Participant -> Coordinator: CommitAck
        }
        abort: {
            Coordinator -> Participant: AbortRequest
            Participant -> Coordinator: AbortAck
        }
    }
}
```

Choreographies define interaction patterns using message passing and control flow constructs. The global view enables reasoning about deadlocks and protocol correctness.

**Local Projection** generates participant-specific implementations from choreographic specifications. For complete details on projection rules and session type generation, see [Choreography System Reference](302_choreography_system.md#rumpsteak-integration).

```rust
// Generated coordinator implementation - uses effect system for infrastructure
// For complete AuraEffectSystem documentation, see System Architecture guide
pub async fn coordinator_role(
    transaction_id: u64,
    participant_id: DeviceId,
    effects: &AuraEffectSystem,
) -> Result<CommitResult, ProtocolError> {
    // Send prepare request
    let prepare_msg = PrepareRequest { transaction_id };
    effects.send_message(participant_id, prepare_msg).await?;

    // Receive prepare response
    let response: PrepareResponse = effects.receive_message().await?;

    // Act on vote
    match response.vote {
        Vote::Commit => {
            let commit_msg = CommitRequest { transaction_id };
            effects.send_message(participant_id, commit_msg).await?;

            let _ack: CommitAck = effects.receive_message().await?;
            Ok(CommitResult::Committed)
        }
        Vote::Abort => {
            let abort_msg = AbortRequest { transaction_id };
            effects.send_message(participant_id, abort_msg).await?;

            let _ack: AbortAck = effects.receive_message().await?;
            Ok(CommitResult::Aborted)
        }
    }
}
```

Local projection produces implementations that handle only the messages and decisions relevant to each role. This eliminates global state and reduces implementation complexity.

**Session Types** provide compile-time guarantees about protocol adherence. For detailed session type documentation and type safety guarantees, see [Choreography System Reference](302_choreography_system.md#protocol-implementation).

```rust
// Session type for coordinator role
type CoordinatorSession = Send<PrepareRequest, Receive<PrepareResponse, Choice<
    Send<CommitRequest, Receive<CommitAck, End>>,
    Send<AbortRequest, Receive<AbortAck, End>>
>>>;

// Session type for participant role
type ParticipantSession = Receive<PrepareRequest, Send<PrepareResponse, Offer<
    Receive<CommitRequest, Send<CommitAck, End>>,
    Receive<AbortRequest, Send<AbortAck, End>>
>>>;
```

Session types encode the communication protocol in the type system. Incorrect message ordering or missing messages result in compile-time errors.

**CRDT Integration with Builder Pattern** enables choreographies to synchronize distributed state using ergonomic setup methods:

```rust
use aura_protocol::effects::semilattice::CrdtCoordinator;
use aura_protocol::choreography::protocols::anti_entropy::execute_anti_entropy;

// Create coordinator using builder pattern
let coordinator = CrdtCoordinator::with_cv_state(device_id, journal_state);

// Execute choreography with CRDT synchronization
let (result, updated_coordinator) = execute_anti_entropy(
    device_id,
    config,
    is_requester,
    &effect_system,
    coordinator,
).await?;

// Extract synchronized state
let synced_state = updated_coordinator.cv_handler().get_state();
```

The builder pattern provides three ergonomic approaches for CRDT handler setup:

```rust
// Approach 1: Convenience methods for common cases
let cv_coordinator = CrdtCoordinator::with_cv(device_id);
let delta_coordinator = CrdtCoordinator::with_delta_threshold(device_id, 100);
let mv_coordinator = CrdtCoordinator::with_mv_state(device_id, constraints);

// Approach 2: Explicit state initialization
let coordinator = CrdtCoordinator::with_cv_state(device_id, initial_journal);

// Approach 3: Multiple handlers chained together
let coordinator = CrdtCoordinator::new(device_id)
    .with_cv_handler(CvHandler::new())
    .with_delta_handler(DeltaHandler::with_threshold(50))
    .with_mv_handler(MvHandler::with_state(caps));
```

For complete CRDT programming patterns and semilattice implementation details, see [CRDT Programming Guide](802_crdt_programming_guide.md).

## Protocol Composition

**Sequential Composition** chains protocols to create complex multi-phase interactions. Sequential composition enables building sophisticated workflows from simpler protocol components.

```rust
// Note: Protocol composition is achieved through manual implementation coordination
// in the current Aura system, not through DSL syntax.

choreography SecureDataTransfer {
    roles: Alice, Bob

    // Phase 1: Authentication messages
    Alice -> Bob: AuthRequest
    Bob -> Alice: AuthResponse

    // Phase 2: Key exchange messages
    Alice -> Bob: KeyRequest
    Bob -> Alice: KeyResponse

    // Phase 3: Data transfer messages
    Alice -> Bob: DataMessage
    Bob -> Alice: DataAck
}
```

Sequential composition executes protocols in order with shared context. Later protocols can access results from earlier protocols through the composition interface.

**Parallel Composition** executes multiple protocols concurrently with synchronization points. Parallel composition enables efficient resource utilization and reduces latency.

```rust
// Note: Parallel execution is handled in the implementation, not the DSL.
// The DSL describes message ordering; parallelism is an implementation detail.

choreography DistributedConsensus {
    roles: Node1, Node2, Node3

    // Proposal phase (implementation can parallelize these)
    Node1 -> Node2: Proposal
    Node1 -> Node3: Proposal
    Node2 -> Node1: Proposal
    Node2 -> Node3: Proposal
    Node3 -> Node1: Proposal
    Node3 -> Node2: Proposal

    // Decision phase
    choice Node1 {
        decide: {
            Node1 -> Node2: Decision
            Node1 -> Node3: Decision
        }
    }
}
```

Parallel composition allows concurrent message exchange followed by synchronization points. Barriers ensure all parallel branches complete before continuing.

**Conditional Composition** selects protocols based on runtime conditions. Conditional composition enables adaptive behavior based on environmental factors or participant capabilities.

```rust
choreography! {
    protocol AdaptiveSync {
        roles: Client, Server;

        Client -> Server: CapabilityQuery;
        Server -> Client: CapabilityResponse(capabilities);

        match capabilities.sync_type {
            SyncType::FullSync => compose FullSynchronization(Client, Server),
            SyncType::DeltaSync => compose DeltaSynchronization(Client, Server),
            SyncType::NoSync => compose Acknowledgment(Client, Server),
        }
    }
}
```

Conditional composition selects appropriate sub-protocols based on participant capabilities or environmental conditions. This enables efficient adaptation to different scenarios.

## Error Handling and Resilience

**Timeout Handling** prevents indefinite blocking when participants fail or networks partition. Timeouts enable graceful degradation and error recovery.

```rust
choreography! {
    protocol RobustRequest {
        roles: Client, Server;

        Client -> Server: Request(data);

        timeout(30_seconds) {
            Server -> Client: Response(result);
        } or {
            Client: HandleTimeout();
        }
    }
}
```

Timeout specifications define maximum waiting periods for message exchanges. Timeout handlers provide fallback behavior when communication fails.

**Retry Mechanisms** handle transient failures by re-attempting failed operations. Retry mechanisms improve protocol reliability under unreliable network conditions.

```rust
pub async fn robust_coordinator(
    transaction_id: u64,
    participant_id: DeviceId,
    effects: &AuraEffectSystem,
) -> Result<CommitResult, ProtocolError> {
    for attempt in 1..=3 {
        match attempt_commit_protocol(transaction_id, participant_id, effects).await {
            Ok(result) => return Ok(result),
            Err(ProtocolError::Timeout) if attempt < 3 => {
                // Exponential backoff
                let delay = Duration::from_millis(100 * 2_u64.pow(attempt - 1));
                tokio::time::sleep(delay).await;
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Err(ProtocolError::MaxRetriesExceeded)
}
```

Retry logic implements exponential backoff to avoid overwhelming failed participants. Retry attempts distinguish between recoverable and permanent failures.

**Compensation Protocols** undo partial progress when protocols fail midway through execution. Compensation enables maintaining consistency despite failures.

```rust
choreography! {
    protocol CompensatingTransaction {
        roles: Coordinator, ServiceA, ServiceB;

        // Forward operations
        try {
            Coordinator -> ServiceA: ReserveResource(resource_id);
            ServiceA -> Coordinator: ReservationConfirmed;

            Coordinator -> ServiceB: AllocateResource(resource_id);
            ServiceB -> Coordinator: AllocationConfirmed;

            Coordinator -> ServiceA: CommitReservation(resource_id);
            Coordinator -> ServiceB: CommitAllocation(resource_id);
        } compensate {
            // Compensation operations in reverse order
            Coordinator -> ServiceB: ReleaseAllocation(resource_id);
            Coordinator -> ServiceA: ReleaseReservation(resource_id);
        }
    }
}
```

Compensation protocols define cleanup operations that execute when forward operations fail. Compensation maintains system consistency by undoing partial changes.

**Participant Recovery** handles situations where participants crash and restart during protocol execution. Recovery mechanisms enable protocols to continue after participant failures.

```rust
#[derive(Debug, Clone)]
pub struct ProtocolCheckpoint {
    pub protocol_id: String,
    pub phase: ProtocolPhase,
    pub state: ProtocolState,
    pub timestamp: u64,
}

pub async fn recover_participant_state(
    device_id: DeviceId,
    protocol_id: &str,
    effects: &AuraEffectSystem,
) -> Result<Option<ProtocolCheckpoint>, RecoveryError> {
    let checkpoint_key = format!("protocol:{}:{}", protocol_id, device_id);

    match effects.storage_load(&checkpoint_key).await {
        Ok(data) => {
            let checkpoint: ProtocolCheckpoint = bincode::deserialize(&data)?;
            Ok(Some(checkpoint))
        }
        Err(StorageError::NotFound) => Ok(None),
        Err(e) => Err(RecoveryError::Storage(e)),
    }
}
```

Recovery mechanisms restore participant state from persistent checkpoints. Participants can rejoin protocols at appropriate phases after recovery.

## Testing Distributed Protocols

**Deterministic Testing** validates protocol correctness using controlled environments. Deterministic tests eliminate network variability and enable reproducible validation.

```rust
#[tokio::test]
async fn test_two_phase_commit_success() {
    let coordinator_id = DeviceId::new();
    let participant_id = DeviceId::new();

    // For AuraEffectSystem testing patterns, see System Architecture guide
    let network = MockNetwork::deterministic();
    let coordinator_effects = AuraEffectSystem::with_network(network.clone());
    let participant_effects = AuraEffectSystem::with_network(network.clone());

    let transaction_id = 12345;

    // Run coordinator and participant concurrently
    let (coordinator_result, participant_result) = tokio::join!(
        coordinator_role(transaction_id, participant_id, &coordinator_effects),
        participant_role(transaction_id, Vote::Commit, &participant_effects)
    );

    assert!(matches!(coordinator_result.unwrap(), CommitResult::Committed));
    assert!(participant_result.is_ok());
}
```

Deterministic testing uses mock networks that provide predictable message delivery. Tests validate protocol behavior without external dependencies.

**Property-Based Testing** validates protocol properties using randomly generated inputs. Property tests discover edge cases and validate invariants across many scenarios.

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_threshold_signing_safety(
        signers in prop::collection::vec(any::<DeviceId>(), 3..10),
        threshold in 2usize..7,
        message in prop::collection::vec(any::<u8>(), 32..128)
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let result = run_threshold_signing_protocol(signers, threshold, message).await;

            // Safety: Valid threshold signatures must verify
            if let Some(signature) = result.threshold_signature {
                prop_assert!(verify_threshold_signature(&message, &signature, threshold));
            }

            // Safety: Cannot generate signature with fewer than threshold signers
            if result.participating_signers.len() < threshold {
                prop_assert!(result.threshold_signature.is_none());
            }
        });
    }
}
```

Property-based tests validate safety and liveness properties across many random protocol executions. This approach discovers rare failure conditions.

**Chaos Testing** validates protocol resilience by injecting failures during execution. Chaos testing ensures protocols handle adverse conditions gracefully.

```rust
pub struct ChaosNetwork {
    inner: Arc<dyn NetworkEffects>,
    failure_rate: f64,
    delay_range: Range<Duration>,
    partition_probability: f64,
}

#[async_trait]
impl NetworkEffects for ChaosNetwork {
    async fn send_message<T: Serialize + Send>(
        &self,
        recipient: DeviceId,
        message: T,
    ) -> Result<(), NetworkError> {
        // Inject random delays
        let delay = self.random_delay();
        tokio::time::sleep(delay).await;

        // Inject random failures
        if self.should_fail() {
            return Err(NetworkError::MessageLost);
        }

        // Inject network partitions
        if self.is_partitioned(recipient) {
            return Err(NetworkError::NetworkPartition);
        }

        self.inner.send_message(recipient, message).await
    }
}
```

Chaos testing injects realistic failure scenarios to validate protocol robustness. This approach discovers bugs that occur under stress conditions.

**Model-Based Testing** validates protocol implementations against formal specifications. Model-based testing ensures implementations conform to theoretical protocol definitions.

```rust
pub fn validate_protocol_trace(
    trace: &ProtocolTrace,
    spec: &ProtocolSpecification,
) -> Result<(), ValidationError> {
    let mut state_machine = spec.initial_state();

    for event in &trace.events {
        match state_machine.process_event(event) {
            Ok(new_state) => state_machine = new_state,
            Err(e) => return Err(ValidationError::InvalidTransition {
                event: event.clone(),
                state: state_machine.clone(),
                error: e,
            }),
        }
    }

    if !state_machine.is_terminal() {
        return Err(ValidationError::IncompleteExecution);
    }

    Ok(())
}
```

Model-based testing compares execution traces against formal protocol specifications. This ensures implementations correctly follow protocol semantics.

## Advanced Protocol Patterns

**Multi-Party Protocols** coordinate interactions between multiple participants with complex dependencies. Multi-party protocols enable sophisticated distributed applications.

```rust
choreography! {
    protocol DistributedAuction {
        roles: Auctioneer, Bidder1, Bidder2, Bidder3;

        // Auction announcement
        Auctioneer -> Bidder1: AuctionAnnouncement(item);
        Auctioneer -> Bidder2: AuctionAnnouncement(item);
        Auctioneer -> Bidder3: AuctionAnnouncement(item);

        // Bidding rounds
        for round in 1..max_rounds {
            parallel {
                Bidder1 -> Auctioneer: Bid(amount1);
                Bidder2 -> Auctioneer: Bid(amount2);
                Bidder3 -> Auctioneer: Bid(amount3);
            }

            let highest_bid = max(amount1, amount2, amount3);

            Auctioneer -> Bidder1: RoundResult(highest_bid);
            Auctioneer -> Bidder2: RoundResult(highest_bid);
            Auctioneer -> Bidder3: RoundResult(highest_bid);
        }

        // Winner announcement
        Auctioneer -> Bidder1: AuctionResult(winner);
        Auctioneer -> Bidder2: AuctionResult(winner);
        Auctioneer -> Bidder3: AuctionResult(winner);
    }
}
```

Multi-party choreographies coordinate complex interactions with synchronization and decision points. These protocols enable building sophisticated distributed applications.

**Streaming Protocols** handle continuous data flows between participants. Streaming protocols provide efficient communication for real-time applications.

```rust
choreography! {
    protocol DataStreaming {
        roles: Producer, Consumer;

        Producer -> Consumer: StreamInit(stream_id);
        Consumer -> Producer: StreamAck;

        loop {
            Producer -> Consumer: DataChunk(chunk);
            Consumer -> Producer: ChunkAck(chunk_id);

            if chunk.is_last {
                break;
            }
        }

        Producer -> Consumer: StreamEnd;
        Consumer -> Producer: StreamComplete;
    }
}
```

Streaming protocols provide flow control and acknowledgment mechanisms for efficient data transfer. These protocols handle backpressure and ensure reliable delivery.

**Hierarchical Protocols** organize complex systems using protocol composition and delegation. Hierarchical protocols enable building scalable distributed systems.

```rust
choreography! {
    protocol HierarchicalConsensus {
        roles: Leader, Replica1, Replica2, SubLeader, SubReplica1, SubReplica2;

        // Top-level consensus
        compose Consensus(Leader, Replica1, Replica2);

        // Sub-group consensus
        compose Consensus(SubLeader, SubReplica1, SubReplica2);

        // Cross-level coordination
        Leader -> SubLeader: CoordinationMessage(decision);
        SubLeader -> Leader: CoordinationAck;
    }
}
```

Hierarchical protocols combine multiple consensus groups with coordination protocols. This approach enables scaling to large numbers of participants through structured organization.
