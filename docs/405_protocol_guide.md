# Developer Guide: Creating Distributed Protocols in Aura

This guide provides comprehensive instructions for implementing distributed protocols in Aura using the choreographic DSL, session type algebra, and effect system architecture.

## Overview

Aura's protocol development follows a **unified layered architecture**:

1. **Session Type Algebra** (`docs/401_session_type_algebra.md`) - Global protocol structure
2. **Choreographic DSL** (`work/rumpsteak_aura.md`) - High-level protocol definition
3. **Effect System** (`docs/400_effect_system.md`) - Execution substrate
4. **Semilattice Types** (`docs/402_crdt_types.md`, `docs/403_meet_semi_lattice.md`) - State management

This guide shows how these layers work together to create safe, composable distributed protocols.

## Quick Start: Writing Your First Protocol

### Step 1: Define the Global Choreography

Use the `rumpsteak-aura` choreographic DSL to define your protocol from a global perspective:

```rust
use rumpsteak_choreography::choreography;

choreography! {
    SimpleHandshake {
        roles: Client, Server
        
        // Phase 1: Client initiates
        Client -> Server: Hello(client_id)
        
        // Phase 2: Server responds
        Server -> Client: Welcome(session_id)
        
        // Phase 3: Client acknowledges
        Client -> Server: Ready
    }
}
```

### Step 2: Define Message Types

Create strongly-typed messages in `aura-types/src/identifiers.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandshakeMessage {
    Hello { client_id: DeviceId },
    Welcome { session_id: SessionId },
    Ready,
}
```

### Step 3: Execute the Protocol

Use Aura's unified effect system to execute the choreography:

```rust
use aura_choreography::integration::{create_testing_adapter, create_choreography_endpoint};
use aura_protocol::ChoreographicRole;

// Create unified choreography adapter
let mut adapter = create_testing_adapter(device_id);

// Create endpoint for this device's role
let role = ChoreographicRole::new(device_id, 0);
let endpoint = create_choreography_endpoint(device_id, role, adapter.context().clone());

// Execute choreography
SimpleHandshake::execute(&mut adapter, &mut endpoint).await?;
```

## Choreographic DSL Reference

### Basic Communication Patterns

#### Point-to-Point Send
```rust
Alice -> Bob: Message
```
- Alice sends a message to Bob
- Creates `Effect::Send` for Alice, `Effect::Recv` for Bob

#### Broadcast
```rust
Leader ->* : Announcement
```
- Leader sends to all other roles
- Expands to multiple individual sends during projection

#### Sequential Composition
```rust
Alice -> Bob: Request
Bob -> Alice: Response
Charlie -> Dave: Data
```
- Operations execute in order
- Sequential composition via continuations

### Branching and Choice

#### Simple Choice
```rust
choice Client {
    buy: {
        Client -> Server: Purchase(item)
        Server -> Client: Receipt
    }
    cancel: {
        Client -> Server: Cancel
    }
}
```

#### Guarded Choice
```rust
choice Client {
    buy when (balance > price): {
        Client -> Server: Purchase(item)
        Server -> Client: Receipt  
    }
    insufficient_funds when (balance <= price): {
        Client -> Server: InsufficientFunds
    }
    cancel: {
        Client -> Server: Cancel
    }
}
```

### Parallel Composition

```rust
parallel {
    Alice -> Bob: Data1
    Charlie -> Dave: Data2
    Eve -> Frank: Data3
}
```

**Safety Rule**: No conflicts allowed - each role can only send/receive to/from one other role per parallel branch.

### Loops and Recursion

#### Fixed Count Loop
```rust
loop (count: 5) {
    Client -> Server: Request
    Server -> Client: Response
}
```

#### Role-Controlled Loop
```rust
loop (decides: Client) {
    Client -> Server: MoreData
    Server -> Client: Ack
    // Client decides whether to continue
}
```

#### Custom Condition Loop
```rust
loop (custom: "has_more_batches") {
    Processor -> Coordinator: BatchResult
    Coordinator -> Processor: NextBatch
}
```

#### Recursive Protocols
```rust
rec DataStream {
    Producer -> Consumer: Data
    choice Consumer {
        continue: {
            Consumer -> Producer: More
            // Recursively call DataStream
        }
        finish: {
            Consumer -> Producer: Done
        }
    }
}
```

### Sub-Protocol Composition

```rust
choreography! {
    ComplexProtocol {
        roles: A, B, C
        
        protocol Handshake {
            A -> B: Hello
            B -> A: Welcome
        }
        
        protocol DataExchange {
            A -> B: Data
            B -> C: ProcessedData
        }
        
        // Main protocol
        call Handshake
        call DataExchange
        A -> C: Complete
    }
}
```

## Integration with Aura Effect System

### Using Effects in Protocols

Protocols execute through Aura's unified effect system. Effects provide the primitive operations that choreographies coordinate:

```rust
// Effects are used internally by choreographic execution
async fn execute_handshake<H: AuraHandler>(handler: &H) -> Result<(), Error> {
    // NetworkEffects for communication
    let message = handler.receive_message().await?;
    
    // CryptoEffects for security
    let signature = handler.sign_message(&message).await?;
    
    // StorageEffects for persistence
    handler.store_session_data(&session_id, &data).await?;
    
    Ok(())
}
```

### Effect Handler Injection

Choose appropriate effect handlers for your execution context:

```rust
// Testing with mock effects
let handler = AuraEffectSystem::for_testing(device_id);

// Production with real effects  
let handler = AuraEffectSystem::for_production(device_id)?;

// Simulation with controlled effects
let handler = AuraEffectSystem::for_simulation(device_id, seed);
```

### Using Middleware

Add cross-cutting concerns through middleware:

```rust
// Base handler
let base = AuraEffectSystem::for_production(device_id)?;

// Add middleware for resilience and observability
let with_retry = RetryMiddleware::new(base, 3);
let with_metrics = MetricsMiddleware::new(with_retry);
let with_tracing = TracingMiddleware::new(with_metrics, "dkd-protocol");

// Execute protocol with enhanced handler
execute_protocol(&with_tracing).await?;
```

## Using Semilattice Systems

### Join-Based CRDTs for State Synchronization

For protocols that need eventual consistency with accumulative semantics:

```rust
use aura_types::semilattice::{StateMsg, CvState};
use aura_protocol::effects::semilattice::CvHandler;
use aura_choreography::semilattice::execute_cv_sync;

choreography! {
    StateSynchronization {
        roles: Replica[N]
        
        loop (custom: "sync_interval") {
            loop (count: N) {
                Replica[i] ->* : StateUpdate(state_delta)
            }
        }
    }
}

// Execute with join-based CRDT handler
let mut cv_handler = CvHandler::<JournalMap>::new();
execute_cv_sync(adapter, replicas, my_role, &mut cv_handler).await?;
```

### Meet-Based CRDTs for Constraint Satisfaction

For protocols that need constraint intersection and capability restriction:

```rust
use aura_types::semilattice::{MeetStateMsg, ConstraintMsg, MvState};
use aura_protocol::effects::semilattice::MvHandler;
use aura_choreography::semilattice::execute_constraint_sync;

choreography! {
    CapabilityRestriction {
        roles: Enforcer[N]
        
        // Each enforcer proposes constraints
        loop (count: N) {
            Enforcer[i] ->* : ConstraintMsg(capability_constraint)
        }
        
        // Compute intersection of all constraints
        loop (count: N) {
            Enforcer[i].local_meet_computation()
        }
        
        // Verify consistency
        loop (count: N) {
            Enforcer[i] ->* : ConsistencyProof(verification)
        }
    }
}

// Execute with meet-based CRDT handler
let mut mv_handler = MvHandler::<CapabilitySet>::new();
execute_constraint_sync(adapter, constraint, participants, my_device_id).await?;
```

### When to Use Join vs Meet Semilattices

**Use Join Semilattices (CvRDT) when:**
- State grows monotonically (counters, sets, logs)
- Conflict resolution through accumulation
- Eventually consistent replication
- Examples: GCounter, OR-Set, journal entries

**Use Meet Semilattices (MvRDT) when:**
- Constraints become more restrictive over time
- Capability intersection and access control
- Security policy composition
- Examples: CapabilitySet, TimeWindow, SecurityPolicy

## Session Type Safety and Projection

### Understanding Global vs Local Types

**Global Type** (choreographic view):
```
Client → Server: Request . Server → Client: Response . end
```

**Local Type for Client**:
```
! Request . ? Response . end
```

**Local Type for Server**:
```
? Request . ! Response . end
```

### Projection Rules

1. **Send Projection**: Sender gets `!T`, receiver gets `?T`, others skip
2. **Choice Projection**: Decider gets `⊕{...}` (internal choice), others get `&{...}` (external choice)
3. **Parallel Projection**: Check for conflicts, then merge safely
4. **Recursion Projection**: Project body, preserve recursion if non-empty

### Deadlock Prevention

Session types prevent deadlocks through:

- **Duality**: Complementary send/receive operations
- **Linearity**: Each channel used exactly once per protocol
- **Progress**: Guarded recursion ensures advancement
- **Conflict Detection**: Parallel composition checked for safety

## Common Protocol Patterns

### Request-Response
```rust
choreography! {
    RequestResponse {
        roles: Client, Server
        Client -> Server: Request(data)
        Server -> Client: Response(result)
    }
}
```

### Threshold Protocols
```rust
choreography! {
    ThresholdConsensus {
        roles: Participant[N]
        
        // Commitment phase
        loop (count: N) {
            Participant[i] ->* : Commitment(commit_value)
        }
        
        // Reveal phase  
        loop (count: N) {
            Participant[i] ->* : Reveal(reveal_value)
        }
        
        // Verification phase
        loop (count: N) {
            Participant[i].local_verification()
        }
    }
}
```

### Leader Election
```rust
choreography! {
    LeaderElection {
        roles: Candidate[N]
        
        // Nomination phase
        loop (count: N) {
            choice Candidate[i] {
                nominate when (wants_to_lead): {
                    Candidate[i] ->* : Nomination(priority)
                }
                abstain: {
                    // Do nothing
                }
            }
        }
        
        // Selection phase
        loop (count: N) {
            Candidate[i].local_selection()
        }
        
        // Announcement phase
        Candidate[leader] ->* : LeaderAnnouncement
    }
}
```

### Gossip Dissemination
```rust
choreography! {
    GossipProtocol {
        roles: Node[N]
        
        rec GossipRound {
            choice Node[initiator] {
                gossip when (has_updates): {
                    Node[initiator] -> Node[target]: GossipMessage(updates)
                    Node[target] -> Node[initiator]: GossipResponse(ack)
                }
                idle: {
                    // Wait for next round
                }
            }
        }
    }
}
```

## Error Handling and Fault Tolerance

### Protocol-Level Error Handling

```rust
choreography! {
    RobustProtocol {
        roles: Client, Server
        
        Client -> Server: Request(data)
        
        choice Server {
            success when (can_process): {
                Server -> Client: Success(result)
            }
            error when (validation_failed): {
                Server -> Client: ValidationError(details)
            }
            timeout: {
                Server -> Client: TimeoutError
            }
        }
        
        choice Client {
            retry when (can_retry): {
                // Recursive retry
                call RobustProtocol
            }
            abort: {
                Client -> Server: AbortRequest
            }
        }
    }
}
```

### Effect-Level Resilience

Use middleware for automatic retry and fault tolerance:

```rust
let resilient_handler = AuraEffectSystem::for_production(device_id)?
    .with_retry(RetryMiddleware::new(3))
    .with_circuit_breaker(CircuitBreakerMiddleware::new())
    .with_timeout(TimeoutMiddleware::new(Duration::from_secs(30)));
```

## Testing Distributed Protocols

### Unit Testing with Mock Effects

```rust
#[tokio::test]
async fn test_handshake_protocol() {
    let mut client_handler = AuraEffectSystem::for_testing(client_device_id);
    let mut server_handler = AuraEffectSystem::for_testing(server_device_id);
    
    // Test the protocol execution
    let result = execute_handshake(&client_handler, &server_handler).await;
    assert!(result.is_ok());
}
```

### Integration Testing with Simulation

```rust
#[tokio::test]
async fn test_threshold_protocol() {
    let simulator = NetworkSimulator::new()
        .with_participants(5)
        .with_threshold(3)
        .with_network_delays(Duration::from_millis(100));
    
    let result = simulator.execute_threshold_protocol().await;
    assert_eq!(result.consensus_value, expected_value);
}
```

### Property-Based Testing

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn protocol_always_terminates(seed in any::<u64>()) {
        let handler = AuraEffectSystem::for_simulation(device_id, seed);
        let result = tokio_test::block_on(async {
            timeout(Duration::from_secs(10), execute_protocol(&handler)).await
        });
        prop_assert!(result.is_ok());
    }
}
```

## Performance Considerations

### Zero-Cost Abstractions

Session types and choreographic DSL compile to efficient code:

```rust
// High-level choreographic DSL
Alice -> Bob: Request

// Compiles to efficient effects
Effect::Send { to: Bob, msg: Request }

// Executes as direct function calls (zero overhead)
handler.send_message(Bob, Request).await
```

### Message Serialization

Use efficient serialization for protocol messages:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolMessage {
    #[serde(with = "borsh")]  // Use borsh for compact binary
    LargePayload(Vec<u8>),
    
    #[serde(with = "postcard")]  // Use postcard for embedded
    SmallMessage { id: u32, data: String },
}
```

### Batching and Pipelining

Use parallel composition for non-conflicting operations:

```rust
choreography! {
    EfficientBatch {
        roles: Coordinator, Worker[N]
        
        // Parallel task assignment (no conflicts)
        parallel {
            Coordinator -> Worker[1]: Task(batch_1)
            Coordinator -> Worker[2]: Task(batch_2)
            Coordinator -> Worker[3]: Task(batch_3)
        }
        
        // Parallel result collection
        parallel {
            Worker[1] -> Coordinator: Result(output_1)
            Worker[2] -> Coordinator: Result(output_2)
            Worker[3] -> Coordinator: Result(output_3)
        }
    }
}
```

## File Organization and Architecture

When implementing protocols in Aura, follow this organization:

```
crates/
├── aura-types/src/                      # Foundation types (workspace-wide)
│   ├── identifiers.rs                   # Message types, DeviceId, SessionId
│   ├── sessions.rs                      # Protocol AST, projection functions
│   ├── effects/choreographic.rs         # Effect algebra types
│   └── semilattice/                     # CRDT foundation traits
│
├── aura-protocol/src/effects/           # Effect handlers and middleware
│   ├── network.rs                       # NetworkEffects implementation
│   ├── crypto.rs                        # CryptoEffects implementation
│   └── semilattice/                     # CRDT effect handlers
│
├── aura-choreography/src/               # Protocol definitions and execution
│   ├── protocols/                       # Choreographic protocol definitions
│   │   ├── dkd.rs                       # Deterministic key derivation
│   │   ├── frost.rs                     # FROST threshold signatures
│   │   └── consensus.rs                 # Consensus protocols
│   ├── semilattice/                     # CRDT choreographies
│   └── integration.rs                   # Unified choreography adapters
│
└── aura-<domain>/src/                   # Domain-specific logic
    ├── protocols/                       # Domain protocols
    └── types.rs                         # Domain-specific message types
```

## Best Practices

### Protocol Design

1. **Start Global**: Always design from the global choreographic perspective first
2. **Type Safety**: Use strongly-typed messages with clear semantics
3. **Deadlock Free**: Verify projection safety through testing
4. **Idempotent**: Design operations to be safely retryable
5. **Versioned**: Include version information in protocol messages

### Error Handling

1. **Explicit Failure Modes**: Model all failure conditions in choreographies
2. **Graceful Degradation**: Provide fallback paths for partial failures
3. **Timeout Handling**: Include explicit timeout branches
4. **Recovery Protocols**: Design protocols for recovering from failures

### Performance

1. **Minimize Rounds**: Reduce communication rounds where possible
2. **Batch Operations**: Use parallel composition for independent operations
3. **Stream Large Data**: Use streaming for large payloads
4. **Compress Messages**: Use efficient serialization formats

### Testing

1. **Property-Based**: Test algebraic properties of your protocols
2. **Fault Injection**: Test with simulated network failures
3. **Load Testing**: Verify performance under realistic conditions
4. **Deterministic**: Use simulation for reproducible testing

## Advanced Topics

### Custom Effect Types

For domain-specific effects, implement custom effect traits:

```rust
#[async_trait]
pub trait BiometricEffects {
    async fn verify_fingerprint(&self, template: &[u8]) -> bool;
    async fn capture_face_image(&self) -> Option<Vec<u8>>;
}

// Use in choreographies via effect handlers
```

### Protocol Composition

Compose larger protocols from smaller ones:

```rust
choreography! {
    CompleteProtocol {
        roles: A, B, C
        
        call AuthenticationProtocol    // Sub-protocol 1
        call DataExchangeProtocol      // Sub-protocol 2  
        call FinalizationProtocol      // Sub-protocol 3
    }
}
```

### Dynamic Roles

Handle variable numbers of participants:

```rust
choreography! {
    DynamicConsensus {
        roles: Participant[N]  // N determined at runtime
        
        loop (count: N) {
            Participant[i] ->* : Vote(preference)
        }
        
        // Tally and announce result
        Participant[leader] ->* : Result(consensus)
    }
}
```

## Accessing Rumpsteak-Aura Documentation

### Using the DeepWiki MCP Server

For detailed information about the Rumpsteak-Aura choreography DSL, use the DeepWiki MCP server to query the documentation:

```rust
// Query the complete documentation structure
mcp__deepwiki__read_wiki_structure("hxrts/rumpsteak-aura")

// Read the full documentation content
mcp__deepwiki__read_wiki_contents("hxrts/rumpsteak-aura")

// Ask specific questions about the DSL
mcp__deepwiki__ask_question("hxrts/rumpsteak-aura", "How do I write parallel choreographies?")
mcp__deepwiki__ask_question("hxrts/rumpsteak-aura", "What are the projection rules for choice statements?")
mcp__deepwiki__ask_question("hxrts/rumpsteak-aura", "How do I use guards in choice branches?")
```

**Key areas to query:**
- **Choreography DSL**: Syntax for writing global protocols
- **Session Type System**: How session types ensure compile-time safety
- **Algebraic Effect Interfaces**: How generated code exposes effect interfaces
- **WASM Build Process**: Building for WebAssembly targets
- **Projection Rules**: How global types become local session types
- **Error Handling**: Managing failures in choreographic protocols

**Example queries:**
```rust
// Understanding the DSL syntax
"How do I write choice statements with guards?"
"What's the syntax for recursive protocols?"
"How do I handle variable numbers of participants?"

// Session type theory
"How does projection work for parallel composition?"
"What safety guarantees do session types provide?"
"How are conflicts detected in parallel branches?"

// Implementation details
"How do I integrate choreographies with custom effect handlers?"
"What's the difference between Effect::Choose and Effect::Offer?"
"How do I debug choreographic protocol execution?"
```

The DeepWiki integration provides up-to-date, searchable access to the complete Rumpsteak-Aura documentation, including examples, API references, and theoretical foundations.

## Related Documentation

- **[Session Type Algebra](docs/401_session_type_algebra.md)** - Formal foundations
- **[Effect System Architecture](docs/400_effect_system.md)** - Execution substrate
- **[CRDT Types](docs/402_crdt_types.md)** - State-based protocols
- **[Meet Semi-Lattice](docs/403_meet_semi_lattice.md)** - Constraint-based protocols
- **[Rumpsteak-Aura DSL](work/rumpsteak_aura.md)** - Complete DSL reference

## Troubleshooting

### Common Compilation Errors

**Projection conflicts in parallel composition:**
```
Error: InconsistentParallel - Alice sends to Bob in multiple branches
```
Fix: Ensure each role only communicates with one other role per parallel branch.

**Unguarded recursion:**
```
Error: Infinite recursion detected
```
Fix: Add termination conditions or use loop statements with explicit bounds.

**Type mismatches:**
```
Error: Expected Message type, found String  
```
Fix: Ensure message types match between send and receive operations.

### Runtime Issues

**Network connectivity issues:**
- Check network handler configuration
- Verify role discovery and addressing
- Test with controlled network conditions first

**Performance bottlenecks:**
- Profile effect handler implementations
- Use middleware sparingly in hot paths
- Consider batching for high-frequency operations

This developer guide provides the foundation for implementing robust, type-safe distributed protocols in Aura. The combination of choreographic programming, session types, and algebraic effects enables both safety and expressivity for complex distributed systems.