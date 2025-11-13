# Aura Crate Dependency Graph and API Reference

This document provides a comprehensive overview of the Aura project's crate structure, dependencies, and exposed APIs.

## Workspace Structure

```
crates/
├── aura-agent           Main agent entry point and device runtime
├── aura-authenticate    Device, threshold, and guardian authentication protocols
├── aura-cli             Command-line interface for account management
├── aura-core            Foundation types (ID system, effects, semilattice, config)
├── aura-crypto          Crypto primitives (FROST, HPKE, key derivation, middleware)
├── aura-effects         Standard effect handler implementations (the standard library)
├── aura-frost           FROST threshold signatures and key resharing (TEMPORARILY EXCLUDED)
├── aura-invitation      Invitation and acceptance choreographies
├── aura-journal         CRDT-based authenticated ledger for account state
├── aura-mpst            Multi-party session types and choreographic specifications
├── aura-protocol        Unified effect system and middleware architecture
├── aura-quint-api       Quint formal verification integration
├── aura-recovery        Guardian recovery and account recovery choreographies
├── aura-rendezvous      Social Bulletin Board peer discovery and routing
├── aura-simulator       Deterministic simulation engine with chaos testing
├── aura-storage         High-level storage orchestration and search
├── aura-store           Capability-driven encrypted chunk storage
├── aura-sync            CRDT synchronization protocols and anti-entropy
├── aura-testkit         Shared testing utilities, mocks, fixtures
├── aura-transport       P2P communication with middleware-based architecture
├── aura-verify          Signature verification and identity validation
└── aura-wot             Web-of-trust capability system with meet-semilattice
```

## Dependency Graph

```mermaid
graph TD
    %% Foundation Layer
    types[aura-core]

    %% Cryptography Layer
    crypto[aura-crypto]
    verify[aura-verify]

    %% Implementation Layer (Standard Library)
    effects[aura-effects]

    %% Type System & Protocol Specs
    mpst[aura-mpst]

    %% Protocol Infrastructure
    journal[aura-journal]
    protocol[aura-protocol]
    wot[aura-wot]
    sync[aura-sync]

    %% Storage & Transport
    transport[aura-transport]
    store[aura-store]

    %% Authentication & Recovery
    auth[aura-authenticate]
    recovery[aura-recovery]
    invitation[aura-invitation]
    frost[aura-frost]

    %% Peer Discovery
    rendezvous[aura-rendezvous]

    %% Application Layer
    agent[aura-agent]
    storage[aura-storage]

    %% Development Tools
    testkit[aura-testkit]
    cli[aura-cli]

    %% Simulation & Analysis
    aura_quint_api[aura-quint-api]
    simulator[aura-simulator]

    %% Dependencies
    crypto --> types
    verify --> types
    verify --> crypto
    effects --> types
    effects --> crypto
    mpst --> types
    identity --> types
    identity --> crypto
    identity --> mpst
    journal --> types
    journal --> crypto
    transport --> types
    transport --> protocol
    store --> journal
    store --> crypto
    store --> types
    store --> protocol
    protocol --> crypto
    protocol --> journal
    protocol --> types
    protocol --> verify
    protocol --> wot
    protocol --> identity
    protocol --> effects
    wot --> types
    wot --> protocol
    sync --> types
    sync --> mpst
    sync --> journal
    auth --> types
    auth --> mpst
    auth --> verify
    auth --> wot
    recovery --> auth
    recovery --> verify
    recovery --> wot
    recovery --> mpst
    recovery --> protocol
    recovery --> journal
    invitation --> auth
    invitation --> wot
    invitation --> mpst
    invitation --> transport
    frost --> crypto
    frost --> journal
    frost --> mpst
    rendezvous --> transport
    rendezvous --> wot
    rendezvous --> mpst
    storage --> store
    storage --> journal
    agent --> types
    agent --> protocol
    agent --> journal
    agent --> crypto
    agent --> transport
    agent --> store
    agent --> verify
    agent --> wot
    agent --> sync
    agent --> recovery
    agent --> invitation
    agent --> effects
    testkit --> agent
    testkit --> crypto
    testkit --> journal
    testkit --> transport
    testkit --> types
    testkit --> protocol
    cli --> agent
    cli --> protocol
    cli --> types
    cli --> recovery
    simulator --> agent
    simulator --> journal
    simulator --> transport
    simulator --> crypto
    simulator --> protocol
    simulator --> types
    simulator --> aura_quint_api

    %% Styling
    classDef foundation fill:#e1f5fe
    classDef crypto fill:#f3e5f5
    classDef effects fill:#e8f5e8
    classDef types fill:#f8e5f5
    classDef protocol fill:#e8f5e8
    classDef storage fill:#fff8e1
    classDef app fill:#fce4ec
    classDef dev fill:#f1f8e9
    classDef sim fill:#e0f2f1

    class types foundation
    class crypto,verify crypto
    class effects effects
    class mpst,identity types
    class journal,protocol,wot,sync protocol
    class transport,store,storage storage
    class auth,recovery,invitation,frost,rendezvous app
    class agent app
    class testkit,cli dev
    class aura_quint_api,simulator sim
```

## Architecture Layers

### Foundation Layer (Blue)
- **aura-core**: Core shared types and identifiers (types, errors, protocols, sessions, capabilities)

### Cryptography Layer (Purple)
- **aura-crypto**: Cryptographic primitives (FROST, DKD, Ed25519, HPKE)
- **aura-verify**: Signature verification and authentication checking

### Implementation Layer - Standard Library (Light Green)
- **aura-effects**: Context-free, stateless effect handler implementations (mock and real variants)

### Type System & Specification Layer (Light Purple)
- **aura-mpst**: Multiparty session types and choreographic protocol specifications

### Protocol Infrastructure Layer (Green)
- **aura-journal**: CRDT-based authenticated ledger for account state
- **aura-protocol**: Unified effect system and middleware for protocol operations
- **aura-wot**: Web of Trust capability-based authorization with meet-semilattice operations
- **aura-sync**: Synchronization protocols and anti-entropy algorithms

### Storage & Transport Layer (Yellow)
- **aura-transport**: P2P communication with middleware-based architecture
- **aura-store**: Capability-driven encrypted storage with access control

### Authentication Layer (Orange)
- **aura-authenticate**: Device, threshold, and guardian authentication protocols

### Application Layer (Pink)
- **aura-agent**: High-level unified agent API with session types

### Development Tools (Light Green)
- **aura-testkit**: Testing utilities, mocks, and fixtures
- **aura-cli**: Command-line interface for account management

### Simulation & Analysis (Teal)
- **aura-quint-api**: Quint formal verification integration
- **aura-simulator**: Deterministic protocol simulation and testing framework

---

## Crate API Reference

### aura-core
**Purpose**: Core shared types and identifiers - single source of truth for domain concepts

**Key Exports**:
- **Identifiers**: `AccountId`, `DeviceId`, `SessionId`, `EventId`, `GuardianId`
- **Protocol Types**: `ProtocolType` (Dkd, Counter, Resharing, Locking, Recovery, Compaction)
- **Session Types**: `SessionStatus` (Initializing, Active, Waiting, Completed, Failed, Expired, TimedOut, Cancelled)
- **Capabilities**: `CapabilityId`, `CapabilityScope`, `CapabilityResource`, `Permission`
- **Content**: `ContentId`, `ChunkId`, `ManifestId`
- **Peers**: `PeerInfo`, `RelationshipType`, `ContextType`
- **Errors**: `AuraError`, `ErrorCode`, `ErrorSeverity`
- **Semilattice**: `JoinSemiLattice`, `MeetSemiLattice` traits and implementations
- **Configuration System** (`config` module):
  - **Traits**: `AuraConfig`, `ConfigDefaults`, `ConfigMerge`, `ConfigValidation` for unified configuration handling across components
  - **Formats**: `ConfigFormat`, `JsonFormat`, `TomlFormat` for multiple configuration file formats
  - **Loader**: `ConfigLoader`, `ConfigSource`, `ConfigPriority` for hierarchical configuration loading (defaults < file < env < CLI)
  - **Validation**: `ConfigValidator`, `ValidationRule`, `ValidationResult` for compile-time and runtime configuration validation
  - **Builder**: `ConfigBuilder` for fluent configuration creation with merging and validation support
- **Flow Budget**: `FlowBudget`, `Receipt`, `FlowBudgetKey` for privacy budget tracking with hybrid semilattice semantics
- **Causal Context**: `VectorClock`, `CausalContext`, `OperationId` for CRDT causal ordering

**Dependencies**: None (foundation crate)

---

### aura-effects
**Purpose**: Standard effect handler implementations - the standard library for Aura's effect system

**Key Exports**:
- **Basic Handlers**: `RealCryptoHandler`, `MockNetworkHandler`, `MemoryStorageHandler`, `FilesystemStorageHandler`
- **Testing Variants**: Mock implementations for all core effect traits
- **Production Variants**: Real implementations using external libraries
- **Context-Free Operations**: Stateless, single-party effect implementations

**Dependencies**: `aura-core`, external libraries (tokio, blake3, etc.)

**Note**: This is the "standard library" layer - provides basic effect implementations that work in any execution context

---

### aura-crypto
**Purpose**: Cryptographic primitives and threshold cryptography implementation

**Key Exports**:
- **FROST**: Threshold signatures (`FrostSignature`, `FrostKeyShare`)
- **DKD**: Deterministic Key Derivation
- **Encryption**: Ed25519 signatures, HPKE encryption, Blake3 hashing
- **Middleware**: Composable security and audit logging middleware
- **Key Derivation**: Key rotation and secure random generation

**Dependencies**: `aura-core`

---

### aura-verify
**Purpose**: Signature verification and identity validation

**Key Exports**:
- **Verification**: Device signature verification, threshold verification
- **Identity Validation**: Principal and identity checking
- **Authentication Types**: Verification contexts and results
- **Errors**: `VerificationError`

**Dependencies**: `aura-core`, `aura-crypto`

---

### aura-mpst
**Purpose**: Multiparty session types and choreographic protocol specifications

**Key Exports**:
- **Session Types**: Type-safe protocol definitions using rumpsteak-aura DSL
- **Choreography**: Global protocol specifications projected to local views
- **Context Isolation**: Context barriers for privacy and unlinkability
- **Analysis**: Protocol analysis and property checking
- **Leakage Tracking**: Privacy budget and information flow analysis

**Dependencies**: `aura-core`

---

### aura-identity
**Purpose**: Device identity, key derivation, and principal management

**Key Exports**:
- **Device Identity**: Device creation and identification
- **Key Derivation**: Key derivation contexts and paths
- **Principal Management**: Device principal information
- **Authentication Integration**: Identity-based authentication

**Dependencies**: `aura-core`, `aura-crypto`, `aura-mpst`

---

### aura-authenticate
**Purpose**: Device, threshold, and guardian authentication protocols

**Key Exports**:
- **Device Authentication**: Device challenge-response authentication
- **Threshold Authentication**: M-of-N signature verification
- **Guardian Authentication**: Guardian approval workflows
- **Session Authentication**: Session ticket verification
- **Types**: `AuthenticationContext`, `ThresholdConfig`
- **Errors**: `AuthenticationError`

**Dependencies**: `aura-core`, `aura-mpst`, `aura-verify`, `aura-wot`

---

### aura-journal
**Purpose**: CRDT-based authenticated ledger for account state and eventual consistency

**Key Exports**:
- **Core State**: `AccountState`, `AccountLedger`, `Appliable` trait
- **Events**: Protocol event types (threshold-signed operations)
- **Bootstrap**: Account initialization and genesis ceremony
- **Capabilities**: Capability-based authorization
- **CRDT Types**: Convergent and meet-semilattice implementations
- **Synchronization**: Anti-entropy sync operations
- **Errors**: `JournalError`

**Dependencies**: `aura-core`, `aura-crypto`

**Note**: `aura-verify` dependency temporarily disabled due to compilation issues

---

### aura-protocol
**Purpose**: Unified effect system and middleware architecture for protocol operations

**Key Exports**:
- **Effects**: Core effect traits (`CryptoEffects`, `TimeEffects`, `SystemEffects`)
- **Handlers**: Effect handler registry and composition
- **Middleware**: Composable middleware for effects (tracing, metrics, security)
- **Guards**: Guard chain implementation (`SendGuardChain`, `JournalCoupler`)
- **Context**: Protocol execution context
- **Types**: Protocol configuration and error types

**Guard Chain Architecture**:
The guard chain subsystem implements the formal predicate `need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)` through three sequential guards:

CapGuard evaluates authorization by checking whether the required message capability is satisfied by the effective capability set in the current context.

FlowGuard enforces flow budget constraints by verifying headroom and charging the budget before allowing sends. This implements the charge-before-send invariant.

JournalCoupler ensures atomic journal updates by coordinating CRDT operations with protocol execution. Supports both pessimistic and optimistic coupling modes.

The guard chain prevents unauthorized sends, enforces privacy budgets, and maintains journal consistency across distributed protocol operations.

**Dependencies**: `aura-crypto`, `aura-journal`, `aura-core`, `aura-verify`, `aura-wot`, `aura-identity`

---

### aura-wot
**Purpose**: Web of Trust capability-based authorization implementing meet-semilattice operations

**Key Exports**:
- **Capabilities**: `Capability`, `CapabilitySet` with semilattice operations
- **Delegation**: `DelegationChain`, `DelegationLink`
- **Policy**: `Policy`, `PolicyEngine` for capability management
- **Authorization**: Authorization evaluation and enforcement
- **Tokens**: `CapabilityToken`, `CapabilityId`
- **Errors**: `WotError`

**Dependencies**: `aura-core`, `aura-protocol`

---

### aura-sync
**Purpose**: Synchronization protocols and anti-entropy algorithms for distributed consensus

**Key Exports**:
- **Sync Protocols**: Anti-entropy synchronization algorithms
- **Peer Management**: Peer state and selection
- **Reconciliation**: Operation reconciliation and conflict resolution
- **Types**: Sync state and metrics
- **Errors**: `SyncError`

**Dependencies**: `aura-core`, `aura-mpst`, `aura-journal`

---

### aura-transport
**Purpose**: P2P communication layer with middleware-based architecture

**Key Exports**:
- **Core Transport**: `TransportHandler`, `TransportOperation`
- **Middleware System**: Composable middleware stack
- **Network Address**: Unified `NetworkAddress` type (TCP, UDP, Memory, Peer)
- **Types**: Message envelopes and metadata
- **Errors**: `TransportError`

**Dependencies**: `aura-core`, `aura-protocol`

---

### aura-store
**Purpose**: Capability-driven encrypted storage with access control

**Key Exports**:
- **Access Control**: Capability-based access enforcement
- **Content Processing**: Chunking, encryption, erasure coding
- **Manifest**: Object manifest with access specifications
- **Storage**: Chunk store and content indexing
- **Replication**: Replication strategies
- **Errors**: `StoreError`

**Dependencies**: `aura-journal`, `aura-crypto`, `aura-core`, `aura-protocol`

**Note**: `aura-transport` dependency temporarily disabled

---

### aura-recovery
**Purpose**: Guardian-based recovery and account recovery choreographies

**Key Exports**:
- **Guardian Recovery**: `G_recovery` choreography, guardian authentication coordination
- **Recovery Ceremonies**: Device key recovery, account access recovery protocols
- **Emergency Operations**: Freeze/unfreeze emergency protocols
- **Dispute Escalation**: `DisputeEscalationManager` with 4 severity levels and auto-cancel logic
- **Recovery Ledger**: `RecoveryLedger` for persistent audit trails of recovery operations
- **Types**: `GuardianSet`, `RecoveryDispute`, `RecoveryEvidence`, `RecoveryShare`
- **Errors**: `RecoveryError`

**Dependencies**: `aura-core`, `aura-authenticate`, `aura-verify`, `aura-wot`, `aura-mpst`, `aura-protocol`, `aura-journal`

---

### aura-invitation
**Purpose**: Invitation and acceptance choreographies for device and guardian onboarding

**Key Exports**:
- **Invitation Choreography**: `G_invitation` main choreography for relationship establishment
- **Guardian Invitations**: Guardian relationship formation protocols
- **Device Invitations**: Device onboarding and acceptance flows
- **Relationship Formation**: Trust relationship creation and capability delegation
- **Types**: `Relationship`, `RelationshipType`, `TrustLevel`, `RelationshipId`
- **Errors**: `InvitationError`

**Dependencies**: `aura-core`, `aura-authenticate`, `aura-wot`, `aura-mpst`, `aura-transport`

---

### aura-rendezvous
**Purpose**: Social Bulletin Board peer discovery and capability-aware routing

**Key Exports**:
- **SBB Flooding**: Gossip-based peer discovery and message propagation
- **Relationship Encryption**: Encryption context isolation based on relationships
- **Capability-Aware Routing**: Message routing enforcing capability constraints
- **Peer Management**: Peer metadata and relationship tracking
- **Errors**: `RendezvousError`

**Dependencies**: `aura-core`, `aura-transport`, `aura-wot`, `aura-mpst`

---

### aura-frost (TEMPORARILY EXCLUDED)
**Purpose**: FROST threshold signatures and key resharing operations

**Key Exports**:
- **Threshold Signatures**: M-of-N FROST signatures with Ed25519
- **Key Resharing**: Dynamic threshold update protocols for guardian set changes
- **Tree Integration**: Integration with ratchet tree for key consistency
- **Share Management**: Secure key share distribution and aggregation
- **Errors**: `FrostError`

**Dependencies**: `aura-core`, `aura-crypto`, `aura-journal`, `aura-mpst`

**Status**: Currently excluded from workspace build due to frost-ed25519 API compatibility issues

---

### aura-agent
**Purpose**: Unified high-level agent API with session types for compile-time state safety

**Key Exports**:
- **Agent Interface**: `AuraAgent` with device runtime composition
- **Effect System Integration**: Runtime composition with handlers and middleware
- **Maintenance & OTA**: OTA orchestration and garbage collection
- **Operations**: Authorization-aware device operations
- **Configuration**: Agent bootstrap and configuration
- **Errors**: `AgentError`

**Dependencies**: `aura-core`, `aura-protocol`, `aura-journal`, `aura-crypto`, `aura-transport`, `aura-store`, `aura-verify`, `aura-wot`, `aura-sync`, `aura-recovery`, `aura-invitation`, `aura-effects`

**Key Features**:
- **OTA Support**: Soft/hard fork detection with epoch fence enforcement
- **Maintenance**: GC event emission, cache invalidation, snapshot coordination
- **Visualization**: CLI recovery status visualization with box-drawing characters

---

### aura-testkit
**Purpose**: Testing utilities and mocks for development

**Key Exports**:
- **Factories**: Test data factories and fixtures
- **Mocks**: Mock implementations of core traits
- **Assertions**: Testing helpers and assertion macros
- **Crypto Utilities**: Test key and signature generation

**Dependencies**: `aura-agent`, `aura-crypto`, `aura-journal`, `aura-transport`, `aura-core`, `aura-protocol`

---

### aura-cli
**Purpose**: Command-line interface for account management and protocol testing

**Key Exports**:
- **Commands**: CLI command implementations for account and device management
- **Configuration**: CLI configuration and argument parsing
- **Visualization**: Rich terminal formatting with box-drawing characters for status displays
- **Recovery Status**: `format_recovery_evidence()`, `format_recovery_dashboard()` for recovery visualization
- **Utilities**: Development and testing utilities for scenario management
- **Handlers**: `CliHandler` for unified CLI effect system integration
- **Effects**: `CliEffects`, `ConfigEffects`, `OutputEffects` for composable CLI operations

**Dependencies**: `aura-agent`, `aura-protocol`, `aura-core`, `aura-recovery`

**Key Features**:
- Unified effect system for all CLI operations
- Rich terminal visualization for recovery and maintenance states
- Scenario discovery, validation, and execution framework

---

### aura-quint-api
**Purpose**: Quint formal verification integration for protocol specifications

**Key Exports**:
- **Evaluator**: Quint specification evaluator interface
- **Properties**: Property verification utilities
- **Runner**: Quint execution and trace analysis
- **Types**: Quint-specific type definitions

**Dependencies**: None (external integration)

---

### aura-simulator
**Purpose**: Deterministic protocol simulation and testing framework

**Key Exports**:
- **Simulation Engine**: Core simulation runtime with deterministic execution
- **Adversary Models**: Byzantine failure and network attack simulation
- **Analysis**: Trace recording and failure analysis
- **Builder**: Simulation scenario configuration
- **Middleware System**: Property checking, state inspection, chaos injection

**Dependencies**: `aura-agent`, `aura-journal`, `aura-transport`, `aura-crypto`, `aura-protocol`, `aura-core`, `aura-quint-api`

---

## Key Architectural Patterns

1. **Layered Architecture**: Clean separation from foundation types through protocols to applications
2. **Dependency Injection**: Effects system allows injectable side effects for testing
3. **CRDT-Based State**: Eventually consistent state management with semilattice operations
4. **Capability-Based Security**: Meet-semilattice authorization with unified access control
5. **Middleware System**: Composable cross-cutting concerns for effects and transport
6. **Single Source of Truth**: Core types consolidated in aura-core (ProtocolType, SessionStatus, etc.)
7. **Effect System**: Algebraic effects for protocol coordination with composable handlers
8. **Choreographic Programming**: Global protocol specifications with local projections via rumpsteak-aura

---

## Type Consolidation and Single Source of Truth

### ProtocolType Consolidation

**Canonical Definition**: `aura-core::ProtocolType`

**Variants**:
- `Dkd` - Deterministic Key Derivation
- `Counter` - Counter reservation protocol
- `Resharing` - Key resharing for threshold updates
- `Locking` - Resource locking protocol
- `Recovery` - Account recovery protocol
- `Compaction` - Ledger compaction protocol

**Usage Across Crates**:
- `aura-core`: Canonical definition
- `aura-protocol`: Re-exports and uses canonical definition
- `aura-simulator`: Uses canonical definition

---

### SessionStatus Consolidation

**Canonical Definition**: `aura-core::SessionStatus`

**Variants** (lifecycle order):
1. `Initializing` - Session initializing before execution
2. `Active` - Session currently executing
3. `Waiting` - Session waiting for participant responses
4. `Completed` - Session completed successfully
5. `Failed` - Session failed with error
6. `Expired` - Session expired due to timeout
7. `TimedOut` - Session timed out during execution
8. `Cancelled` - Session was cancelled

**Usage Across Crates**:
- `aura-core`: Single source of truth
- `aura-simulator`: Uses canonical definition
- `aura-cli`: Imports from aura-core

---

### Capability System Layering

The capability system intentionally uses **multiple architectural layers**, each serving legitimate purposes:

- **Canonical types** in `aura-core` provide lightweight references
- **Authorization layer** (`aura-wot`) adds policy enforcement features
- **Storage layer** (`aura-store`) implements capability-based access control
- Clear conversion paths enable inter-layer communication

---

## System Architecture Summary

### Foundation Layer
- **aura-core**: Core types, effects, semilattice operations, identifiers, and configuration system
- **aura-crypto**: FROST threshold cryptography, deterministic key derivation, middleware, and composable security stacks
- **aura-transport**: P2P communication layer with middleware architecture and unified network addressing

### Effect System
- **aura-protocol**: Unified stateless effect system with handlers, guard chains, authorization bridges, and capability soundness verification
- **aura-mpst**: Multi-party session types infrastructure with choreographic guards and journal coupling
- **aura-journal**: CRDT-based authenticated ledger with semilattice handlers and ratchet tree compaction

### Security & Privacy
- **aura-verify**: Identity verification and signature verification framework
- **aura-authenticate**: Choreographic authentication framework supporting device, threshold, and guardian authentication
- **aura-wot**: Capability-based authorization system with policy evaluation and meet-semilattice operations

### Application Layer
- **aura-agent**: Agent runtime with handlers, maintenance orchestration, and over-the-air update management
- **aura-cli**: Command-line interface with recovery visualization and scenario framework
- **aura-rendezvous**: Social Bulletin Board with flooding protocols, relationship encryption, and capability-aware routing
- **aura-invitation**: Relationship formation choreographies with invitation and acceptance flows
- **aura-recovery**: Guardian-based recovery system with multi-level dispute escalation and audit trails

### Advanced Features
- **aura-store**: Low-level encrypted storage with capability-based access control and content processing
- **aura-storage**: High-level content management with search capabilities and garbage collection
- **aura-sync**: Anti-entropy synchronization protocols with peer management and reconciliation
- **aura-frost**: Threshold signatures and key resharing operations (temporarily excluded)

### Development Tools
- **aura-simulator**: Deterministic protocol simulation with chaos testing and property verification
- **aura-testkit**: Testing utilities, fixtures, and scenario framework with integration patterns
- **aura-quint-api**: Quint formal verification integration for protocol property checking

## System Features

### Maintenance & OTA
- Garbage collection with statistics tracking and device state cleanup
- Over-the-air updates with soft/hard fork detection and epoch fence enforcement
- Snapshot coordination and blob cleanup operations
- Cache invalidation integration

### Guardian Recovery
- Four-level dispute escalation system (Low, Medium, High, Critical)
- Persistent recovery ledger for audit trails
- Escalation policies with auto-cancel logic
- CLI visualization with recovery status dashboard

### Testing Framework
- Comprehensive integration test suite
- Multi-device coordination testing
- Property-based testing across core systems
- Deterministic simulation with chaos injection

## Technical Notes

1. **Active Workspace**: 22 crates providing full platform functionality
2. **Architecture**: Stateless effect system eliminating deadlock potential through isolated state services
3. **Security Model**: Threshold cryptography with M-of-N guarantees and capability-based access control
4. **Consistency**: CRDT-based eventual consistency with semilattice operations
5. **Communication**: Choreographic protocols with session type safety and deadlock freedom
