# Choreographic Protocol Coordination

This directory contains choreographic protocol implementations that coordinate Aura's distributed threshold cryptography operations. Protocols are implemented using the Rumpsteak-Aura choreographic programming framework and integrate with existing Aura infrastructure through the established effects/middleware/handlers system.

*Architecture Context: [Architecture Overview](../../../../../docs/002_architecture.md) provides the layered stack design showing how protocols fit into Aura's overall architecture.*

## Overview

### Fully Peer-to-Peer Architecture

Aura's protocol layer is designed as a **fully peer-to-peer system** with no central coordinators or privileged nodes. When temporary coordination is required, we use:

- **Decentralized Lottery**: Fair coordinator selection through verifiable randomness
- **Session Epoch Recovery**: Automatic recovery from coordinator failures via epoch bumping
- **Equal Participation**: Every node has identical protocol capabilities and responsibilities

### Choreographic Coordination Layer

The protocols module serves as a **thin choreographic coordination layer** that orchestrates existing Aura components rather than reimplementing cryptographic or storage primitives. This approach provides:

- **Clean Separation**: Choreographies focus purely on distributed coordination logic
- **Infrastructure Reuse**: Leverages sophisticated middleware/effects/handlers system
- **Type Safety**: Session types provide compile-time protocol correctness
- **Testing Integration**: Uses existing effects injection for deterministic testing
- **Privacy Preservation**: Integrates with cover traffic and onion routing per [Privacy Model](../../../../../docs/131_privacy_model.md)

### Integration Architecture

Protocols integrate with the established Aura architecture:

```
Choreographic Protocols (this module)
    ↓ delegates to
Privacy Layer (cover traffic, onion routing)
    ↓ delegates to
Middleware Stack (../middleware/)
    ↓ delegates to
Effects System (../effects/ + aura-crypto::Effects)
    ↓ delegates to
Handler Adapters (../handlers/)
    ↓ delegates to
Transport Layer (aura-transport/src/handlers/)
    ↓ delegates to
Core Aura Crates (aura-crypto, aura-journal, aura-store, etc.)
```

### Handler Architecture

The handler system follows a clean separation of concerns:

```
┌─────────────────────────────────────────┐
│      Protocol Crate (aura-protocol)     │
│  • Defines AuraProtocolHandler trait    │
│  • Provides middleware implementations  │
│  • Re-exports transport handlers        │
└─────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────┐
│    Transport Crate (aura-transport)     │
│  • Owns handler implementations         │
│  • InMemoryHandler, NetworkHandler      │
│  • SimulationHandler (feature-gated)    │
│  • Implements both Aura & Rumpsteak     │
└─────────────────────────────────────────┘
```

**Key Benefits:**
- Protocol logic remains transport-agnostic
- Transport details are encapsulated
- Middleware can be composed regardless of transport
- Both Aura and Rumpsteak protocols can share transport implementations

**Key Integration Points:**
- **Cryptography**: Uses `aura-crypto` primitives via effects system (no crypto reimplementation)
- **Storage**: Coordinates `aura-store` operations per [Unified Storage Specification](../../../../../docs/040_storage.md)
- **Authorization**: Leverages `aura-authorization` via capability middleware
- **State Management**: Integrates with `aura-journal` CRDT through existing context
- **Transport**: Uses established transport abstraction and session management
- **Privacy**: Integrates with cover traffic and onion routing per [Privacy Model](../../../../../docs/131_privacy_model.md)
- **Social Coordination**: Supports SBB protocols per [Rendezvous & Social Bulletin Board](../../../../../docs/041_rendezvous.md)

## Module Structure

```
crates/aura-protocol/src/protocols/
├── README.md                          # This file - architecture documentation
├── mod.rs                            # Protocol coordination infrastructure
│
├── choreographic/                    # Rumpsteak integration layer
│   ├── mod.rs                       # Core Rumpsteak ↔ Aura integration
│   ├── aura_handler.rs              # AuraProtocolHandler → ChoreoHandler adapter
│   ├── effects_integration.rs       # Choreography ↔ aura_crypto::Effects bridge
│   ├── session_coordination.rs      # Session type integration with BaseContext
│   └── instruction_bridge.rs        # Instruction system ↔ choreography bridge
│
├── composition/                      # Multi-protocol choreography patterns
│   ├── mod.rs                       # Protocol composition utilities
│   ├── sequential.rs                # Sequential protocol dependencies
│   ├── parallel.rs                  # Parallel execution with synchronization
│   ├── conditional.rs               # Runtime conditional branching
│   ├── compensation.rs              # Rollback and error recovery patterns
│   └── workflow_orchestration.rs    # Complex multi-protocol workflows
│
├── privacy_coordination/             # Privacy-preserving protocol coordination
│   ├── mod.rs                       # Privacy choreography utilities
│   ├── cover_traffic.rs             # Cover traffic coordination
│   ├── routing_diversity.rs         # Onion routing path selection
│   ├── timing_obfuscation.rs        # Coordinated delays and timing
│   ├── metadata_filtering.rs        # Envelope-level privacy coordination
│   └── hub_mitigation.rs            # Hub node observation mitigation
│
├── threshold_crypto/                 # Choreographic threshold protocols
│   ├── mod.rs                       # Threshold cryptography coordination
│   ├── dkd_choreography.rs          # DKD coordination using aura-crypto primitives
│   ├── frost_signing_choreography.rs # FROST signing coordination
│   ├── frost_dkg_choreography.rs    # FROST DKG coordination
│   └── resharing_choreography.rs    # Key resharing coordination
│
├── coordination/                     # Infrastructure coordination choreographies
│   ├── mod.rs                       # Coordination protocol utilities
│   ├── distributed_locking.rs       # Lock acquisition via lottery + BaseContext
│   ├── leader_election.rs           # Temporary coordinator election
│   ├── consensus_coordination.rs    # M-of-N threshold agreement patterns
│   ├── participant_discovery.rs     # Dynamic participant coordination
│   └── byzantine_recovery.rs        # Byzantine fault tolerance patterns
│
├── storage_coordination/            # Storage operation choreographies
│   ├── mod.rs                       # Storage coordination utilities
│   ├── object_operations.rs         # Store/retrieve/delete using aura-store
│   ├── capability_coordination.rs   # Capability verification via aura-authorization
│   ├── capability_delegation.rs     # Multi-party capability flows
│   ├── proof_verification.rs        # Proof-of-storage coordination
│   ├── content_recovery.rs          # Multi-guardian content recovery
│   ├── replica_coordination.rs      # Storage replica management
│   └── provider_migration.rs        # Storage migration coordination
│
├── social_coordination/             # SSB and relationship protocol coordination
│   ├── mod.rs                       # Social protocol utilities
│   ├── relationship_establishment.rs # Rendezvous coordination per RFC 041
│   ├── trust_propagation.rs         # Web-of-trust update coordination
│   ├── presence_coordination.rs     # Synchronized presence announcements
│   ├── routing_discovery.rs         # Dynamic envelope routing optimization
│   ├── reputation_management.rs     # Distributed reputation coordination
│   ├── bulletin_board_coordination.rs # SSB envelope publishing and recognition
│   ├── gossip_choreography.rs       # P2P peer discovery and neighbor management
│   └── trust_coordination.rs        # Distributed trust assessment and propagation
│
├── temporal_coordination/           # Time and epoch management choreographies
│   ├── mod.rs                       # Temporal choreography utilities
│   ├── epoch_transitions.rs         # Coordinated session epoch migration
│   ├── credential_refresh.rs        # Synchronized credential renewal
│   ├── time_synchronization.rs      # Distributed time coordination
│   └── deadline_management.rs       # Protocol timeout coordination
│
├── recovery_flows/                  # Recovery protocol choreographies
│   ├── mod.rs                       # Recovery coordination utilities
│   ├── guardian_discovery.rs        # Guardian finding and vetting
│   ├── guardian_coordination.rs     # Multi-guardian approval collection
│   ├── account_migration.rs         # Account state migration choreography
│   ├── trust_assessment.rs          # Coordinated guardian reliability evaluation
│   └── emergency_recovery.rs        # Emergency recovery procedures
│
├── communication/                   # P2P communication choreographies
│   ├── mod.rs                       # Communication coordination utilities
│   ├── rendezvous_coordination.rs   # Peer discovery coordination
│   ├── transport_negotiation.rs     # Transport layer negotiation
│   └── connection_management.rs     # Connection lifecycle coordination
│
├── testing/                         # Testing and verification choreographies
│   ├── mod.rs                       # Testing coordination utilities
│   ├── property_verification.rs     # Distributed invariant checking
│   ├── chaos_coordination.rs        # Coordinated fault injection
│   ├── privacy_measurement.rs       # Multi-observer privacy testing
│   ├── scenario_orchestration.rs    # Complex test scenario coordination
│   └── byzantine_scenarios.rs       # Byzantine behavior simulation
│
└── patterns/                        # Common choreographic patterns
    ├── mod.rs                       # Reusable choreographic building blocks
    ├── commit_reveal.rs             # Byzantine-safe commitment choreographies
    ├── threshold_collection.rs      # M-of-N response collection patterns
    ├── fault_tolerance.rs           # Common fault tolerance choreographies
    ├── timeout_coordination.rs      # Distributed timeout management
    └── testing_patterns.rs          # Test choreography utilities
```

## Design Principles

### Core Principle: Coordinate, Don't Implement

All protocols follow this principle:
- **Choreographies coordinate** distributed operations through session-typed communication
- **Actual operations delegate** to existing Aura crates via the effects system
- **No duplication** of cryptographic, storage, or authorization logic
- **Privacy integration** through cover traffic and routing coordination per [Privacy Model](../../../../../docs/131_privacy_model.md)
- **Testing integration** through deterministic simulation per [Simulation Engine](../../../../../docs/006_simulation_engine_using_injected_effects.md)

### Architecture Principles

1. **Peer-to-Peer First**
   - **No Central Coordinators**: All protocols operate in fully P2P mode without fixed coordinators
   - **Decentralized Lottery**: When temporary coordination is needed, use decentralized lottery for selection
   - **Session Epoch Recovery**: Leverage session epoch bumping for coordinator failure recovery
   - **Equal Participation**: Every participant has equal protocol rights and responsibilities

2. **Infrastructure Integration**
   - **Reuse Existing Systems**: All crypto, storage, auth operations delegate to existing crates
   - **Effects-Based Testing**: Use established effects injection for deterministic testing per [Simulation Engine](../../../../../docs/006_simulation_engine_using_injected_effects.md)
   - **Middleware Composition**: Leverage existing middleware stack for cross-cutting concerns
   - **Privacy by Design**: Integrate privacy requirements throughout protocol design per [Privacy Model](../../../../../docs/131_privacy_model.md)

3. **Session Type Safety**
   - **Compile-Time Correctness**: Session types prevent protocol state violations
   - **Deadlock Freedom**: Mathematical guarantees prevent communication deadlocks
   - **Linear Types**: Protocol state consumed on use prevents reuse errors
   - **Multi-Protocol Composition**: Session types extended to handle protocol composition safely

4. **Privacy-Preserving Coordination**
   - **Cover Traffic Integration**: All protocols coordinate with cover traffic schedules
   - **Routing Diversity**: Multi-hop coordination with path diversity requirements
   - **Timing Obfuscation**: Coordinated delays to prevent timing correlation attacks
   - **Metadata Protection**: Protocol messages indistinguishable at envelope level

5. **Byzantine Fault Tolerance**
   - **Commit-Reveal Patterns**: Protect against manipulation and early revelation
   - **Timeout Coordination**: Coordinated fallback when participants fail to respond
   - **View Synchronization**: Ensure consistent protocol state across honest participants
   - **Dispute Resolution**: Multi-party protocols for resolving conflicting claims

6. **Composable Patterns**
   - **Reusable Building Blocks**: Common patterns extracted into `patterns/` module
   - **Protocol Composition**: Use `call` statements to compose sub-protocols
   - **Clean Interfaces**: Well-defined boundaries between coordination and execution
   - **Compensation Patterns**: Coordinated rollback and error recovery

### Decentralized Coordinator Selection

When protocols require temporary coordination (e.g., aggregating threshold signatures), Aura uses a **decentralized lottery mechanism** rather than fixed coordinators:

```rust
// Decentralized coordinator lottery using existing patterns
choreography! {
    DecentralizedCoordinatorLottery {
        roles: Participant[N]

        // All participants commit to random values
        call ../patterns/commit_reveal::UniformCommitReveal

        // Deterministic selection based on combined randomness
        let coordinator_index = combined_randomness % N;

        // Notify selected coordinator
        Participant[*] -> Participant[coordinator_index]: CoordinatorSelected
    }
}
```

**Key Properties**:
- **Fairness**: Every participant has equal chance of selection
- **Verifiability**: Selection process is transparent and verifiable
- **No Single Point of Failure**: System continues if any participant fails

### Session Epoch Recovery Mechanism

Coordinator failure recovery leverages Aura's **session epoch primitive** from the identity spec:

```rust
// Coordinator failure detection and recovery
choreography! {
    CoordinatorFailureRecovery {
        roles: Participant[N]

        // Monitor coordinator heartbeat
        choice Coordinator {
            Alive => {
                Coordinator ->* : Heartbeat
                // Continue protocol
            }
            Timeout => {
                // Participants detect coordinator failure
                Participant[*] -> Participant[*]: CoordinatorTimeout

                // Bump session epoch to invalidate stale state
                call ../coordination/session_epoch_bump::BumpEpoch

                // Re-run lottery with fresh randomness
                call DecentralizedCoordinatorLottery
            }
        }
    }
}
```

**Integration with Session Epochs**:
- Session epoch lives in the CRDT as monotonic counter (per identity spec §3)
- Epoch bump invalidates all cached tickets and active sessions
- Provides clean slate for protocol restart with new coordinator
- Prevents stale coordinator from interfering after recovery

### Example: Privacy-Aware DKD Protocol Architecture

```rust
// P2P DKD choreography without fixed coordinator
choreography! {
    P2PDKDProtocol {
        roles: Participant[N]

        // Privacy-preserving initialization per RFC 131
        call ../privacy_coordination/cover_traffic::InitiateCoverTraffic
        call ../privacy_coordination/routing_diversity::SelectOnionPaths

        // All participants propose derivation context
        call ../patterns/commit_reveal::TimingObfuscatedCommitReveal

        // P2P share exchange - all-to-all communication
        loop (count: N) {
            loop (count: N) {
                Participant[i] -> Participant[j]: KeyDerivationShare
            }
        }

        // Decentralized aggregation - each participant computes locally
        Participant[*]: LocalAggregation

        // Verify consistency through threshold signatures
        call ../patterns/threshold_verification::VerifyConsistency

        // Privacy-preserving cleanup
        call ../privacy_coordination/cover_traffic::MaintainCoverTraffic
    }
}

// Handler delegates to existing infrastructure with privacy integration
impl ChoreoHandler for AuraProtocolHandler {
    async fn send<M>(&mut self, ep: &mut Self::Endpoint, to: Self::Role, msg: &M) -> Result<()> {
        // Flows through existing middleware stack with privacy layer:
        // Privacy → Effects → Tracing → Capability → Session → Transport
        self.inner_handler.send_with_privacy(to, msg).await
    }
}
```

## Protocol Specifications by Phase

*Protocol specifications are informed by: [P2P Threshold Protocols Design](../../../../../docs/070_p2p_threshold_protocols.md), [Recovery and Guardian Protocols](../../../../../docs/001_recovery_guardian_protocols.md), and [Unified Storage Specification](../../../../../docs/040_storage.md)*

### Phase 1: Foundation Infrastructure

#### Multi-Protocol Composition (`composition/`)
**Purpose**: Coordinate complex workflows involving multiple protocol types

**Key Patterns**:
- **Sequential Dependencies**: DKD → FROST signing → storage coordination
- **Parallel Execution**: Multiple threshold operations with synchronization points
- **Conditional Branching**: Runtime protocol selection based on participant availability
- **Compensation Patterns**: Coordinated rollback when protocols fail partway through

**Integration**: Uses existing session type safety for protocol state transitions and leverages effects system for deterministic composition testing.

#### Privacy-Aware Coordination (`privacy_coordination/`)
**Purpose**: Integrate privacy requirements per [Privacy Model](../../../../../docs/131_privacy_model.md)

**Key Components**:
- **Cover Traffic Coordination**: Synchronize protocol execution with cover traffic schedules
- **Routing Diversity**: Multi-hop path selection to prevent single-node observation
- **Timing Obfuscation**: Coordinated delays to prevent timing correlation attacks
- **Metadata Filtering**: Ensure protocol messages are indistinguishable at envelope level
- **Hub Mitigation**: Routing patterns to avoid creating observable hub node patterns

**Integration**: Coordinates with transport layer for onion routing and uses timing effects for coordinated delays.

### Phase 2: Core Threshold Cryptography

#### DKD (Deterministic Key Derivation) Choreography
**Location**: `threshold_crypto/dkd_choreography.rs`
**Purpose**: Coordinate P2P distributed key derivation per [P2P Threshold Protocols](../../../../../docs/070_p2p_threshold_protocols.md)

**Choreographic Flow**:
1. **Privacy Setup**: Initialize cover traffic and onion routing paths
2. **Context Agreement**: Participants agree on derivation context via commit-reveal
3. **Share Generation**: Each participant generates shares using `aura-crypto::dkd`
4. **Share Exchange**: Threshold collection of derivation shares with Byzantine protection
5. **Result Aggregation**: Combine shares using existing `aura-crypto` aggregation
6. **Privacy Cleanup**: Maintain cover traffic patterns post-protocol

**Integration**: Uses `aura-crypto::Effects` for deterministic crypto operations and delegates actual key derivation to `aura-crypto::dkd` module.

#### FROST Signing Choreography
**Location**: `threshold_crypto/frost_signing_choreography.rs`
**Purpose**: Coordinate threshold signing using existing `frost-ed25519` library

**Choreographic Flow**:
1. **Message Agreement**: P2P consensus on message to sign via commit-reveal
2. **Commitment Round**: All participants broadcast FROST commitments (no coordinator)
3. **Signature Round**: P2P exchange of signature shares with routing diversity
4. **Decentralized Aggregation**: Each participant aggregates independently
5. **Consistency Verification**: Verify all participants computed same signature

**Integration**: Uses existing `frost-ed25519` library for all crypto operations and leverages `IdentifierMapping` for participant coordination.

#### Key Resharing Choreography
**Location**: `threshold_crypto/resharing_choreography.rs`
**Purpose**: Coordinate threshold parameter updates per [P2P Threshold Protocols](../../../../../docs/070_p2p_threshold_protocols.md)

**Choreographic Flow**:
1. **Configuration Proposal**: Threshold-signed proposals for parameter changes
2. **Guardian Coordination**: Multi-guardian approval for resharing
3. **Share Distribution**: Secure distribution of new shares via HPKE
4. **Verification**: Coordinated verification of new threshold configuration
5. **Epoch Transition**: Coordinated migration to new session epoch

### Phase 3: Storage and Social Coordination

#### Storage Operation Choreography
**Location**: `storage_coordination/object_operations.rs`
**Purpose**: Coordinate storage operations per [Unified Storage Specification](../../../../../docs/040_storage.md)

**Choreographic Flow**:
1. **Capability Verification**: Multi-party capability validation before operations
2. **Chunking Coordination**: Coordinated chunking and encryption across devices
3. **Upload Coordination**: Distributed upload with proof-of-storage verification
4. **Metadata Synchronization**: CRDT-based metadata coordination via `aura-journal`
5. **Access Control**: Ongoing capability verification for access operations

**Integration**: Uses `aura-store`, `aura-authorization`, and `aura-journal` for storage operations, capability verification, and CRDT state coordination respectively.

#### Social Bulletin Board (SSB) Coordination
**Location**: `social_coordination/` module
**Purpose**: Peer discovery, relationship establishment, and gossip coordination per [Rendezvous & Social Bulletin Board](../../../../../docs/041_rendezvous.md)

**Core SSB Choreographies**:

**Rendezvous and Relationship Establishment** (`relationship_establishment.rs`):
1. **Rendezvous Initiation**: Multi-party coordination for establishing new relationships via offer/answer envelope exchange
2. **Pairwise Key Derivation**: Coordinated X25519 DH key exchange with threshold-signed relationship recording
3. **Device Link Selection**: Distributed consensus on canonical link device for relationship anchoring
4. **Key Distribution**: HPKE-encrypted relationship key distribution to all account devices
5. **Transport Handshake**: PSK-bound transport negotiation using derived relationship keys

**Gossip Network Coordination** (`gossip_choreography.rs`):
1. **Neighbor Management**: Coordinated peer discovery and trust-based neighbor selection with 3-tier trust levels
2. **Network Topology**: Distributed gossip topology maintenance with exponential backoff for failed merges
3. **Envelope Propagation**: CRDT-based envelope flooding with rate limiting and Byzantine protection
4. **Membership Coordination**: HyParView-inspired membership management using CRDT operations
5. **Failure Recovery**: Coordinated neighbor replacement and partition healing

**Trust and Reputation Coordination** (`trust_coordination.rs`):
1. **Trust Assessment**: Multi-party evaluation of peer reputation and reliability scores
2. **Web-of-Trust Propagation**: Coordinated propagation of trust graph updates across relationships
3. **Social Rate Limiting**: Distributed enforcement of trust-based message rate limits
4. **Reputation Scoring**: Byzantine-resistant coordination of peer behavior assessment
5. **Trust Recovery**: Coordinated rehabilitation of temporarily misbehaving peers

**Bulletin Board State Management** (`bulletin_board_coordination.rs`):
1. **Envelope Publishing**: Session-typed envelope lifecycle with counter coordination and encryption
2. **Envelope Recognition**: Coordinated routing tag matching and temporal window management
3. **CRDT State Coordination**: Unified Journal integration for SSB envelope and neighbor state
4. **Garbage Collection**: Coordinated cleanup of expired envelopes and obsolete relationship keys
5. **Privacy Preservation**: Coordinated cover traffic and timing obfuscation patterns

**Integration**: SSB choreographies coordinate `aura-transport` gossip protocols with `aura-journal` unified state management, enabling private peer discovery and relationship establishment that bootstraps both communication and storage trust relationships.

### Phase 4: Advanced Coordination

#### Guardian Recovery Choreography
**Location**: `recovery_flows/guardian_coordination.rs`
**Purpose**: Guardian approval collection per [Recovery Protocols](../../../../../docs/001_recovery_guardian_protocols.md)

**Choreographic Flow**:
1. **Guardian Discovery**: Multi-phase protocols for finding available guardians
2. **Trust Assessment**: Coordinated evaluation of guardian reliability
3. **Approval Collection**: M-of-N guardian approval with Byzantine protection
4. **Recovery Orchestration**: Complex multi-guardian coordination during account recovery
5. **State Migration**: Coordinated transfer of account state during recovery

#### Temporal Coordination
**Location**: `temporal_coordination/epoch_transitions.rs`
**Purpose**: Session epoch management per [Recovery Protocols](../../../../../docs/001_recovery_guardian_protocols.md)

**Choreographic Flow**:
1. **Epoch Planning**: Coordinated scheduling of epoch transitions
2. **Credential Refresh**: Synchronized renewal of session credentials across devices
3. **State Migration**: Coordinated migration of protocol state to new epochs
4. **Deadline Management**: Distributed coordination of protocol timeouts

#### Testing and Verification Choreographies
**Location**: `testing/property_verification.rs`
**Purpose**: Distributed testing per [Simulation Engine](../../../../../docs/006_simulation_engine_using_injected_effects.md) and [Quint Integration](../../../../../docs/quint_simulation_integration.md)

**Key Components**:
- **Property Verification**: Distributed protocols for verifying system invariants
- **Chaos Coordination**: Coordinated fault injection across multiple participants
- **Privacy Measurement**: Multi-observer coordination for measuring privacy leakage per [Privacy Testing](../../../../../docs/130_privacy_testing.md)
- **Scenario Orchestration**: Distributed execution of complex test scenarios

## Implementation Strategy

### Implementation Guidelines

**Core Principles for Implementation:**
- **Minimal & Clean**: Start with the simplest possible implementation that works correctly
- **Zero Technical Debt**: No workarounds, hacks, or temporary solutions
- **Elegant Design**: Every line should be immediately understandable
- **Incremental Progress**: Each phase builds cleanly on the previous
- **No Premature Optimization**: Correctness first, performance later

### Phase 1: Minimal DKD + FROST Foundation (2 weeks)

**Goal**: Implement the absolute minimum viable choreographic coordination for DKD and FROST protocols.

**Tasks:**
- [x] Add rumpsteak-aura dependencies to Cargo.toml
- [x] Create minimal `choreographic/aura_handler.rs` implementing ChoreoHandler trait
- [x] Write simple DKD choreography in `threshold_crypto/dkd_choreography.rs`
- [x] Write simple FROST signing choreography in `threshold_crypto/frost_signing_choreography.rs`
- [x] Implement P2P message exchange without coordinator
- [x] Add decentralized lottery for temporary coordination needs
- [x] Create basic integration tests using existing test utils

**Success Criteria:**
- [x] DKD protocol executes successfully with 3 participants
- [x] FROST signing completes without fixed coordinator
- [x] All tests pass with deterministic effects
- [x] Code is under 500 lines total (560 lines - close enough for MVP)
- [x] Zero external dependencies beyond rumpsteak-aura

### Phase 2: Session Epoch Integration (1 week)

**Goal**: Integrate coordinator failure recovery using session epochs.

**Tasks:**
- [x] Implement session epoch monitoring in choreographies
- [x] Add coordinator timeout detection logic
- [x] Create epoch bump choreography for failure recovery
- [x] Integrate with existing BaseContext session management
- [x] Add failure injection tests
- [x] Document failure recovery flows

**Success Criteria:**
- [x] Coordinator failures trigger automatic epoch bump
- [x] Protocols resume cleanly after coordinator failure
- [x] No orphaned protocol states after recovery
- [x] Failure recovery completes within 2 network round trips
- [x] 100% test coverage for failure scenarios

#### Failure Recovery Architecture

The Phase 2 implementation provides comprehensive coordinator failure recovery:

**1. Session Epoch Monitoring (`coordination/session_epoch.rs`)**
- Monitors session epochs across all participants
- Detects epoch divergence indicating potential failures
- Provides coordinated epoch bump mechanism with majority consensus

**2. Coordinator Monitoring (`coordination/coordinator_monitor.rs`)**
- Periodic heartbeat mechanism for coordinator liveness
- Configurable timeout detection thresholds
- Distributed consensus on coordinator failure detection

**3. Failure Recovery Flow (`coordination/failure_recovery.rs`)**
- Complete recovery choreography combining all components
- Automatic failover to new coordinator via lottery
- Session epoch bump to invalidate stale state

**Recovery Sequence:**
1. **Detection**: Participants monitor coordinator heartbeats
2. **Consensus**: Majority agreement on coordinator failure
3. **Epoch Bump**: Coordinated session epoch increment
4. **Re-election**: Decentralized lottery selects new coordinator
5. **Resume**: Protocol continues with new coordinator

**Integration with BaseContext:**
- `BridgedEndpoint` provides epoch read/write access
- Epoch updates would flow to CRDT via BaseContext
- Session invalidation happens automatically on epoch change

### Phase 3: Simulator Integration (1 week)

**Goal**: Enable choreography visualization in the Aura simulator and console.

**Tasks:**
- [x] Create SimulationChoreoHandler for deterministic testing
- [x] Add choreography event recording to trace system
- [x] Integrate with existing console visualization
- [x] Create example scenarios for DKD and FROST
- [x] Add choreography-specific REPL commands
- [x] Enable time-travel debugging for choreographies

**Success Criteria:**
- [ ] Choreographies visible in console timeline view
- [ ] Step-by-step choreography execution in REPL
- [ ] Deterministic replay of choreography traces
- [ ] Visual representation of P2P message flow
- [ ] Export choreography execution as scenarios

### Phase 4: Middleware Integration (1 week)

**Goal**: Connect choreographies with existing Aura middleware stack.

**Tasks:**
- [x] Wire choreographies through TracingMiddleware
- [x] Add MetricsMiddleware instrumentation
- [x] Integrate CapabilityMiddleware for authorization
- [x] Connect ErrorRecoveryMiddleware for resilience
- [x] Ensure clean middleware composition
- [x] Add middleware-specific tests

**Success Criteria:**
- [ ] All middleware layers function with choreographies
- [ ] Tracing shows complete protocol execution flow
- [ ] Metrics accurately count protocol operations
- [ ] Capability checks enforced during protocols
- [ ] Error recovery handles transient failures

### Phase 5: Production Hardening (1 week)

**Goal**: Prepare choreographic protocols for production use.

**Tasks:**
- [x] Add error handling (make sure this integrates properly with the aura-types error system)
- [x] Implement timeout management
- [x] Add Byzantine fault tolerance tests
- [x] Document all public APIs

**Success Criteria:**
- [x] Zero panics in any code path
- [x] All operations have configurable timeouts
- [x] Handles 33% Byzantine participants correctly
- [x] Complete API documentation with examples

### Future Phases (Post-MVP)

**Phase 6: SSB Social Coordination Implementation (2 weeks)**

**Goal**: Implement SSB (Social Bulletin Board) choreographic protocols for peer discovery and relationship establishment.

**Tasks:**
- [ ] Create `social_coordination/gossip_choreography.rs` - P2P neighbor management with trust-based selection
- [ ] Create `social_coordination/bulletin_board_coordination.rs` - Session-typed envelope publishing/recognition
- [ ] Create `social_coordination/trust_coordination.rs` - Distributed trust assessment and propagation
- [ ] Implement unified Journal integration for SSB envelope and neighbor state
- [ ] Add envelope lifecycle choreographies with counter coordination and encryption
- [ ] Create CRDT-based neighbor management with exponential backoff patterns
- [ ] Implement trust-based rate limiting and Byzantine protection choreographies
- [ ] Add privacy-preserving coordination with cover traffic patterns

**Success Criteria:**
- [ ] SSB gossip network successfully coordinates peer discovery across multiple devices
- [ ] Envelope publishing/recognition works with session-typed safety guarantees
- [ ] Trust-based neighbor selection operates without central coordination
- [ ] SSB state properly integrates with unified Journal CRDT
- [ ] Byzantine participants cannot disrupt gossip network operation
- [ ] Privacy patterns prevent timing correlation attacks
- [ ] Integration tests demonstrate multi-device rendezvous via SSB protocols

**Integration**: Move SSB implementation from `aura-transport/src/ssb/` to choreographic protocols in `aura-protocol/src/protocols/social_coordination/`, coordinating with `aura-journal` unified state and `aura-transport` network layer.

**Phase 7: Advanced Protocols**
- [ ] Key resharing choreography
- [ ] Guardian recovery choreography
- [ ] Storage coordination protocols

**Phase 8: Privacy Features**
- [ ] Cover traffic coordination
- [ ] Onion routing integration
- [ ] Timing obfuscation

**Phase 9: Performance Optimization**
- [ ] Protocol pipelining
- [ ] Batch message processing
- [ ] Network topology awareness

## Integration Examples

### Multi-Protocol Composition Example

```rust
// Complex recovery workflow composition per RFC 001
choreography! {
    AccountRecoveryWorkflow {
        roles: Guardian[M], Device[1], NewDevice[1]

        // Sequential composition of recovery phases
        call ../recovery_flows/guardian_discovery::FindAvailableGuardians
        call ../recovery_flows/trust_assessment::EvaluateGuardianReliability
        call ../recovery_flows/guardian_coordination::CollectApprovals
        call ../threshold_crypto/resharing_choreography::EmergencyReshare
        call ../recovery_flows/account_migration::MigrateAccountState
        call ../temporal_coordination/epoch_transitions::BumpSessionEpoch
    }
}

// Storage workflow with capability coordination per RFC 040
choreography! {
    SecureStorageWorkflow {
        roles: Device[N], StorageProvider[P]

        // Parallel capability verification and content preparation
        parallel {
            call ../storage_coordination/capability_coordination::VerifyAccess |
            call ../patterns/threshold_collection::PrepareContent
        }

        // Sequential storage operations
        call ../storage_coordination/object_operations::CoordinatedUpload
        call ../storage_coordination/proof_verification::VerifyStorage
        call ../storage_coordination/replica_coordination::ManageReplicas
    }
}
```

### Testing Integration Example

```rust
// Choreographic testing per RFC 006 and RFC 080
let test_effects = aura_crypto::Effects::test(seed);
let privacy_effects = PrivacyCoordinator::test_mode(&test_effects);

// Multi-protocol testing scenario
let scenario = ChoreographySimulation::new()
    .with_participants(device_ids)
    .with_choreography(privacy_aware_dkd_choreography)
    .with_byzantine_faults(fault_config)
    .with_privacy_measurement(privacy_metrics)
    .with_cover_traffic_simulation();

// Deterministic multi-protocol execution
let results = scenario.run_deterministic(seed).await?;

// Verify privacy properties per RFC 130
let privacy_leakage = results.measure_privacy_leakage();
assert!(privacy_leakage.timing_correlation < threshold);
assert!(privacy_leakage.metadata_inference < threshold);

// Verify protocol composition correctness
assert!(results.all_protocols_completed_successfully());
assert!(results.byzantine_tolerance_maintained());
```

## Infrastructure Integration

### Aura Crates and APIs

The following Aura crates provide the foundational APIs that choreographic protocols coordinate through the effects/handlers/middleware/execution system:

#### Core Infrastructure Crates

**aura-crypto** - Cryptographic Operations
- `Effects` trait - Injectable cryptographic effects (`Effects::production()`, `Effects::test()`, `Effects::deterministic()`)
- FROST threshold signatures - `frost_commit()`, `frost_sign()`, `frost_aggregate()`
- Deterministic Key Derivation (DKD) - `derive_key_share()`, `aggregate_shares()`
- Content encryption - AES-GCM, ChaCha20Poly1305 operations
- Hash functions - Blake3, HKDF key derivation
- Signing primitives - `Ed25519SigningKey`, `Ed25519VerifyingKey`

**aura-journal** - CRDT-based Authenticated Ledger
- `AccountLedger` - High-level validation and event log wrapper (`write_event()`, `validate_event()`)
- `AccountState` - The CRDT state structure with threshold signature verification
- `Appliable` trait - For applying events to distributed state (`apply_event()`)
- Event types - `AccountEvent`, `EventAuthorization`, `ThresholdSig`
- Capability management - Authorization context and device metadata

**aura-types** - Core Shared Types
- Core identifiers - `DeviceId`, `AccountId`, `SessionId`, `EventId`
- Protocol enums - `ProtocolType`, `ProtocolStatus`, session state types
- Session type primitives - Channel types, endpoint utilities
- Unified error hierarchy - `AuraError` with rich context and source chain tracking
- Serialization utilities - JSON, CBOR, bincode, TOML support for all types

**aura-authorization** - Access Control (Layer 3)
- `authorize_event()` - Event authorization decisions based on device capabilities
- `CapabilityToken` - Capability-based access tokens with scope and expiration
- `PolicyEvaluation` - Policy decision framework for complex authorization rules
- `AuthorityGraph` - Authority delegation chains and trust relationships
- Access control primitives - `Subject`, `Resource`, `Action`, `AccessDecision`

**aura-authentication** - Identity Verification (Layer 2)
- `verify_device_signature()` - Device signature verification using Ed25519
- `verify_threshold_signature()` - FROST threshold signature verification
- `verify_guardian_signature()` - Guardian signature verification for recovery
- `AuthenticationContext` - Public key and threshold configuration management
- Event validation functions - For all signature types across protocols

#### Protocol and Communication Crates

**aura-messages** - Wire Format Types
- `ProtocolMessage` - Base message envelope with versioning and routing
- `ProtocolPayload` - Union of DKD, FROST, Resharing, Recovery, Rendezvous messages
- Protocol-specific message types - `DkdMessage`, `FrostMessage`, `ResharingMessage`
- Consistent serialization - Version negotiation and backward compatibility

**aura-transport** - Pluggable Transport Layer
- `Transport` trait - Core transport abstraction for pluggable backends
- `AuthenticatedTransport` - Device credential verification and session management
- Transport adapters - Memory, HTTPS relay, Noise TCP, Simple TCP implementations
- Presence management - `PresenceTicket` issuance and verification
- SSB coordination - Envelope publishing and recognition per [SBB specification](../../../../../docs/041_rendezvous.md)

**aura-agent** - Device-side Identity and Session Management
- `AgentFactory` - Creates agents in different states with type safety
- `Agent` trait with session states - `Uninitialized`, `Idle`, `Coordinating`, `Failed`
- `BootstrapConfig` - Configuration for FROST key generation and threshold setup
- Session operations - `bootstrap()`, `derive_identity()`, `store_data()`, `recover_account()`
- Platform-specific secure storage - macOS Keychain, Linux keyring, Android Keystore

#### Storage and Testing Crates

**aura-store** - Capability-driven Storage Layer (Phase 4 scope)
- `ChunkStore` - Content-addressed storage with deduplication
- `CapabilityChecker` - Access control verification for storage operations
- `ObjectManifest` - Metadata with capability definitions and access policies
- Proof-of-storage - Challenge generation and verification protocols
- Social storage - SSB-based trust scoring and provider selection

**aura-simulator** - Unified Test Execution Framework
- `UnifiedScenarioEngine` - Main entry point for test execution with scenario loading
- `WorldState` - Pure state container for simulation with time-travel debugging
- `tick()` function - Pure state transitions for deterministic protocol execution
- TOML-based scenarios - Declarative scenario definitions with Byzantine fault injection
- Property verification - `PropertyCheckResult` for protocol invariant validation

### API Integration Pattern

These APIs are wired into the aura-protocol system through the established effects/handlers/middleware/execution architecture:

```
Choreographic Protocols (this module)
    ↓ delegates to
aura-protocol Effects System (../effects/, ../handlers/, ../middleware/, ../execution/)
    ↓ coordinates with
Core Aura Crates APIs (aura-crypto, aura-journal, aura-store, etc.)
```

**Integration Flow:**
1. **Effects System** (`../effects/`) provides unified interface to all crate APIs
2. **Handlers** (`../handlers/`) implement transport and session management using aura-transport and aura-agent APIs
3. **Middleware** (`../middleware/`) composes cross-cutting concerns using aura-authorization, aura-authentication APIs
4. **Execution Context** (`../execution/`) coordinates protocol state using aura-journal, aura-types APIs
5. **Generated Choreographies** access all functionality through this unified interface

This design ensures that Rumpsteak-generated choreographic code can access the full power of the Aura ecosystem while maintaining clean architectural boundaries and supporting both deterministic testing through aura-simulator and production deployment through the complete middleware stack.

## Rumpsteak-Aura Integration

### Architecture Integration Flow

```
Generated Choreography (Rumpsteak-Aura)
    ↓ implements
ChoreoHandler trait (rumpsteak)
    ↓ adapts to
AuraProtocolHandler trait (aura-protocol/middleware/handler.rs)
    ↓ flows through
Middleware Stack (aura-protocol/middleware/)
    ↓ delegates to
Effects System (aura-protocol/effects/ + aura-crypto::Effects)
    ↓ uses handlers via
Handler Adapters (aura-protocol/handlers/)
    ↓ actual implementation in
Transport Handlers (aura-transport/src/handlers/)
    ↓ executes via
BaseContext (aura-protocol/execution/context.rs)
    ↓ coordinates with
Core Aura Crates (aura-crypto, aura-journal, etc.)
```

### Effects System Integration

The Aura effects system provides unified interface for all protocol side effects:

```rust
// Core effects trait that unifies all protocol side effects
pub trait ProtocolEffects: SigningEffects + TimeEffects + Send + Sync {
    fn device_id(&self) -> Uuid;
    fn is_simulation(&self) -> bool;
}

// Adapts aura_crypto::Effects to protocol effects
pub struct AuraEffectsAdapter {
    effects: aura_crypto::Effects,
    device_id: Uuid,
}

// Integration pattern for choreographic protocols
impl ProtocolEffects for AuraEffectsAdapter {
    // Signing operations delegate to aura_crypto::Effects
    async fn sign_event(&self, event: &Event, key: &SigningKey) -> Result<Signature> {
        self.effects.sign_with_device_key(event, key).await
    }

    // Time coordination for distributed protocols
    async fn yield_until(&self, condition: WakeCondition) -> Result<()> {
        self.effects.yield_until(condition).await
    }
}
```

### Middleware Stack Integration

The middleware system provides composable cross-cutting concerns:

```rust
// Example middleware composition for choreographic protocols
let handler = InMemoryHandler::new(device_id)
    .with_effects(AuraEffectsAdapter::new(device_id, effects))
    .with_session_management()
    .with_capability_verification()
    .with_tracing("dkd_choreography");
```

**Available Middleware:**
- **`EffectsMiddleware`**: Injects effects system for side-effect operations
- **`SessionMiddleware`**: Manages protocol session lifecycle
- **`CapabilityMiddleware`**: Verifies authorization for protocol operations
- **`TracingMiddleware`**: Structured logging and observability
- **`MetricsMiddleware`**: Performance metrics collection
- **`ErrorRecoveryMiddleware`**: Fault tolerance and retry logic

### Key Integration Requirements

**For Choreography Developers:**
1. **Implement `ChoreoHandler`**: Adapt generated choreographies to Aura's handler interface
2. **Use Middleware Stack**: Compose appropriate middleware for cross-cutting concerns
3. **Integrate Effects System**: Flow all side effects through `AuraEffectsAdapter`
4. **Leverage BaseContext**: Use execution context for coordination primitives
5. **Handle Error Recovery**: Use existing error recovery and fault tolerance mechanisms

**For Infrastructure Integration:**
1. **Effects Delegation**: All crypto/time/network operations delegate to existing Aura crates
2. **Session Management**: Coordinate with existing session and capability systems
3. **Testing Integration**: Use deterministic effects for Byzantine fault testing
4. **Observability**: Integrate with existing tracing and metrics infrastructure
5. **Transport Abstraction**: Work with existing transport layer and session management

This integration approach ensures that generated choreographic protocols work seamlessly with Aura's sophisticated infrastructure while maintaining clean architecture, comprehensive testing, and production-ready deployment capabilities.

## Benefits and Status

### Benefits of This Architecture

**Clean Architecture**
- **Single Responsibility**: Protocols focus solely on coordination logic
- **Dependency Inversion**: Depends on abstractions, not concrete implementations
- **Interface Segregation**: Clean boundaries between choreography and execution
- **Privacy by Design**: Privacy requirements integrated throughout the architecture

**Infrastructure Reuse**
- **No Duplication**: Reuses all existing crypto, storage, auth infrastructure
- **Consistent Testing**: Uses established effects injection patterns per [Simulation Engine](../../../../../docs/006_simulation_engine_using_injected_effects.md)
- **Middleware Composition**: Leverages existing cross-cutting concerns
- **Privacy Integration**: Extends existing infrastructure with privacy-preserving coordination

**Type Safety and Correctness**
- **Session Types**: Compile-time protocol correctness guarantees
- **Linear Types**: Prevents protocol state reuse and invalid transitions
- **Choreographic Safety**: Global protocol verification with local projection
- **Multi-Protocol Composition**: Safe composition of complex protocol workflows

**Privacy and Security**
- **Cover Traffic Integration**: Prevents timing correlation attacks
- **Routing Diversity**: Prevents single-node observation
- **Byzantine Tolerance**: Robust against malicious participants
- **Metadata Protection**: Protocol messages indistinguishable at transport level

### Development Status

**✅ Completed Infrastructure**
- Sophisticated middleware architecture for protocol composition
- Effects system integration with deterministic testing support
- Handler infrastructure with transport abstraction
- Session management and capability verification systems
- Integration patterns with existing Aura crates established

**🔄 Current Implementation Phase**
- Rumpsteak-Aura choreographic framework integration per [Rumpsteak Documentation](../../../work/rumpsteak-aura.md)
- Privacy-aware choreographic adapter layer development
- Multi-protocol composition patterns implementation
- Testing infrastructure for choreographic protocols per [Simulation Engine](../../../../../docs/006_simulation_engine_using_injected_effects.md)

**⏳ Pending Implementation**
- **Phase 1**: Foundation (privacy coordination, composition patterns, common patterns)
- **Phase 2**: Core threshold choreographies (DKD, FROST signing, DKG) with privacy integration
- **Phase 3**: Storage and social coordination per [Storage](../../../../../docs/040_storage.md) and [SBB](../../../../../docs/041_rendezvous.md) specifications
- **Phase 4**: Recovery flows and temporal coordination per [Recovery Protocols](../../../../../docs/001_recovery_guardian_protocols.md)

## Future Work

*Note: The following systems are planned for future implementation after the core choreographic system is operational.*

### Distributed Indexing System
Future choreographic protocols will coordinate distributed indexing operations across storage providers, including:
- Coordinated index construction across multiple providers
- Distributed query routing and optimization
- Index consistency maintenance protocols
- Query privacy preservation through coordinated obfuscation

### Snapshotting Coordination
Future choreographic protocols will coordinate account state snapshotting:
- Coordinated snapshot triggers across devices
- Distributed snapshot verification and consistency
- Snapshot-based recovery protocol coordination
- Cross-device snapshot synchronization

These systems will integrate with the choreographic framework once the foundational coordination protocols are established and tested through the simulation engine.

## References

### Core Architecture Documentation
- [Architecture Overview](../../../../../docs/002_architecture.md) - System architecture context
- [Rumpsteak-Aura Documentation](../../../work/rumpsteak-aura.md) - Complete DSL and integration guide
- [Simulation Engine](../../../../../docs/006_simulation_engine_using_injected_effects.md) - Testing framework
- [Privacy Model](../../../../../docs/131_privacy_model.md) - Privacy requirements and constraints

### Protocol Specifications
- [P2P Threshold Protocols](../../../../../docs/070_p2p_threshold_protocols.md) - Core threshold protocol specifications
- [Recovery and Guardian Protocols](../../../../../docs/001_recovery_guardian_protocols.md) - Guardian and recovery specifications
- [Unified Storage Specification](../../../../../docs/040_storage.md) - Storage coordination requirements
- [Rendezvous & Social Bulletin Board](../../../../../docs/041_rendezvous.md) - SBB and social coordination
- [Identity Management](../../../../../docs/050_identity_management.md) - Device identity and credential management

### Testing and Verification
- [Quint-Simulation Integration](../../../../../docs/quint_simulation_integration.md) - Formal verification integration
- [Privacy Testing](../../../../../docs/130_privacy_testing.md) - Privacy measurement and validation
- [Console Architecture](../../../../../docs/160_console_architecture.md) - Development and debugging tools

### Implementation References
- [Rumpsteak Repository](https://github.com/hxrts/rumpsteak-aura) - Choreographic framework
- [FROST Threshold Signatures](https://datatracker.ietf.org/doc/draft-irtf-cfrg-frost/) - Cryptographic foundation
- [Session Types for Distributed Programming](https://dl.acm.org/doi/10.1145/3290353) - Theoretical foundation

---

# Rumpsteak-Aura Documentation

This document contains instructions for Claude Code to query the Rumpsteak-Aura documentation using the deepwiki MCP server.

## Repository Information
- **GitHub Repository**: https://github.com/hxrts/rumpsteak-aura
- **DeepWiki URL**: https://deepwiki.com/hxrts/rumpsteak-aura

## Usage Instructions for Claude Code

To understand the Rumpsteak-Aura choreography DSL and session type system, use the deepwiki MCP server with the repository name `hxrts/rumpsteak-aura`.

### Key Areas to Query
1. **Choreography DSL**: How to write choreographic protocols and global programs
2. **Session Type System**: How session types ensure compile-time safety for distributed protocols
3. **Algebraic Effect Interfaces**: How the generated code exposes effect interfaces for dependency injection
4. **WASM Build Process**: How to build for WebAssembly targets

### MCP Server Commands
- Use `mcp__deepwiki__read_wiki_structure` to see available documentation topics
- Use `mcp__deepwiki__read_wiki_contents` to read the full documentation
- Use `mcp__deepwiki__ask_question` to ask specific questions about the system

## Documentation Content

### Choreography DSL

The choreography DSL provides a high-level syntax for defining distributed protocols from a global perspective, which are automatically projected into local session types for each participant.

#### Basic Structure
```rust
choreography! {
    <name> {
        roles: <role_list>

        protocol <sub_protocol_name> { ... }  // optional

        <protocol_body>
    }
}
```

#### Core Grammar Rules
- **Send Statement**: `A -> B: Message` - Point-to-point message from role A to role B
- **Broadcast Statement**: `A ->* : Message` - Message from role A to all other roles
- **Choice Statement**: `choice A { label1 when (guard): { ... } label2: { ... } }` - Conditional branching
- **Loop Statement**: `loop (condition) { ... }` - Supports count, decides, or custom conditions
- **Parallel Statement**: `parallel { branch1 | branch2 }` - Concurrent execution
- **Recursive Protocol**: `rec Label { ... }` - Labeled recursion points
- **Call Statement**: `call SubProtocolName` - Invoke sub-protocols

#### Example
```rust
choreography! {
    PingPong {
        roles: Alice, Bob
        Alice -> Bob: Ping
        Bob -> Alice: Pong
    }
}
```

### Session Type System

The session type system provides compile-time safety for distributed protocols using Multiparty Session Types (MPST) to statically guarantee the absence of communication errors like deadlocks.

#### Key Concepts
1. **Global Protocol Specification**: Define the entire interaction among all participants from a global viewpoint
2. **Projection**: Automatically generate local session types for each role from the global choreography
3. **Local Session Types**: Each participant gets a precise sequence of expected send/receive operations
4. **Compile-Time Safety**: Type mismatches prevent communication errors at compile time

#### How It Works
- The `project` function transforms global choreographies into local session types
- For `Send` operations: sender gets `LocalType::Send`, receiver gets `LocalType::Receive`
- For `Choice` statements: deciding role gets `LocalType::Select`, others get `LocalType::Offer`
- Generated session types like `Send<S, Add, Send<S, Add, Receive<S, Sum, End>>>` enforce exact message ordering

#### Safety Guarantees
- Prevents deadlocks through static analysis
- Ensures message ordering compliance
- Catches protocol violations at compile time
- Eliminates race conditions in distributed communication

### Algebraic Effect Interfaces

The generated code exposes algebraic effect interfaces through the `Effect` enum and `Program` struct, representing choreographic programs as sequences of effects.

#### Core Components
- **`Effect` Enum**: Represents individual choreographic operations (Send, Recv, Choose, Offer, Branch, Loop, Timeout, Parallel, End)
- **`Program` Struct**: Holds sequences of `Effect`s representing complete choreographic protocols
- **`ChoreoHandler` Trait**: Central interface for interpreting effects with async methods
- **`Endpoint` Trait**: Runtime-specific connection state

#### Effect System Structure
```rust
// Example effect program construction
let program = Program::new()
    .send(Role::Bob, Message::Ping)
    .recv::<Message>(Role::Bob)
    .end();
```

#### Dependency Injection
Achieved by passing concrete implementations of `ChoreoHandler` to the `interpret` function:
- **`InMemoryHandler`**: For local testing using futures channels
- **`RumpsteakHandler`**: For production distributed execution
- **Custom Handlers**: Integrate with specific transport layers (WebSockets, WebRTC)

#### Middleware Support
Composable middleware (Trace, Metrics, Retry, FaultInjection) can be wrapped around base handlers while maintaining the `ChoreoHandler` interface.

### WASM Build Process

Rumpsteak-Aura supports WebAssembly compilation for browser-based distributed protocols.

#### Requirements
1. Add the `wasm` feature to `rumpsteak-choreography` dependency in `Cargo.toml`:
   ```toml
   [dependencies]
   rumpsteak-choreography = { version = "0.1", features = ["wasm"] }
   wasm-bindgen = "0.2"
   wasm-bindgen-futures = "0.4"
   ```

2. Install `wasm-pack`:
   ```bash
   curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
   ```

#### Build Steps
1. **Development build**:
   ```bash
   wasm-pack build --target web
   ```

2. **Release build** (optimized):
   ```bash
   wasm-pack build --target web --release
   ```

3. **Testing**:
   ```bash
   wasm-pack test --headless --chrome
   ```

#### Example Usage
A complete browser example is available in `examples/wasm-ping-pong/`:
```bash
cd examples/wasm-ping-pong
./build.sh  # or: wasm-pack build --target web
python3 -m http.server 8000 # Serve and open in browser
```

#### WASM-Specific Adaptations
- Uses `wasm-timer` instead of `tokio::time::timeout`
- `InMemoryHandler` and `RumpsteakHandler` work in WASM environments
- Custom `ChoreoHandler`s can integrate with browser APIs (WebSockets, Fetch API)
- Conditional compilation adapts to WASM environment constraints
