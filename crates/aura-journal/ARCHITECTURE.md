# Aura Journal (Layer 2) - Architecture and Invariants

## Purpose

Define fact-based journal semantics using join-semilattice CRDTs, enabling deterministic conflict-free state reduction across distributed replicas.

## Position in 8-Layer Architecture

**Layer 2: Specification (Domain Crate)**
- Depends only on `aura-core` (Layer 1)
- Provides domain logic for journal semantics (no effect handlers)
- Implements `JournalEffects` application effect trait by composing infrastructure effects
- Used by Layer 4+ (orchestration, feature implementation, runtime)

## Core Modules

### Fact Model (`fact.rs`)

**Purpose**: Immutable, ordered journal entries representing state changes.

**Types**:
- `Journal`: Join-semilattice CRDT with fact set union semantics
- `JournalNamespace`: Scoping (Authority or Context)
- `Fact`: Timestamped fact with metadata (agreement, propagation, ack tracking)
- `FactContent`: Enum of fact types:
  - `AttestedOp`: Commitment tree operations
  - `Relational`: Cross-authority coordination facts
  - `Snapshot`: GC checkpoints
  - `RendezvousReceipt`: Message flow tracking
- `RelationalFact`: Two-tier fact system:
  - `Protocol(ProtocolRelationalFact)`: Core protocol facts in aura-journal
  - `Generic { context_id, envelope }`: Extensibility point for domain facts
- `ProtocolRelationalFact`: Guardian bindings, recovery grants, consensus results, AMP channel state, DKG transcripts, convergence certs, leakage events
- `AckStorage`: Separate acknowledgment tracking (not part of CRDT)

**Key Operations**:
- `Journal::join()` / `join_assign()`: Set union merge
- `add_fact()`, `add_fact_with_options()`: Fact insertion
- Filtering: `ack_tracked_facts()`, `provisional_facts()`, `finalized_facts()`
- Metadata updates: `update_agreement()`, `update_propagation()`, `clear_ack_tracking()`

### Reduction (`reduction.rs`)

**Purpose**: Deterministic state derivation from facts.

**Functions**:
- `reduce_authority(journal) -> TreeStateSummary`: Derive authority tree state from AttestedOps
- `reduce_context(journal, registry) -> RelationalState`: Derive relational state from RelationalFacts

**State Types**:
- `RelationalState`: Bindings derived from relational facts
- `ChannelEpochState`: AMP channel epoch tracking (checkpoints, bumps, policy)

**Invariants**:
- Same fact set → identical state (all replicas)
- Namespace validation (Authority journal → tree state, Context journal → relational state)

### Extensibility (`extensibility.rs`)

**Purpose**: Open/Closed Principle for domain-specific facts.

**Traits**:
- `DomainFact`: Domain fact serialization/deserialization via `FactEnvelope`
  - Used by **Layer 4/5 feature crates only** (aura-chat, aura-invitation, aura-relational, aura-recovery, aura-social)
  - Provides: `type_id()`, `schema_version()`, `context_id()`, `to_envelope()`, `from_envelope()`, `to_generic()`
- `FactReducer`: Reduce domain facts to `RelationalBinding`
  - `handles_type()`, `reduce_envelope()`
- `FactRegistry`: Runtime fact type registration
  - Maps type_id → BoxedReducer
  - Used by `aura-agent/src/fact_registry.rs`

**Layer 2 vs Layer 4/5 Pattern**:
- **Layer 2 domain crates** (aura-maintenance, aura-authorization, aura-signature, aura-store, aura-transport):
  - Use `aura_core::types::facts` pattern
  - NO dependency on aura-journal
  - NO registration in FactRegistry
  - Create `RelationalFact::Generic` manually at usage sites
- **Layer 4/5 feature crates**:
  - Use `DomainFact` trait + `#[derive(DomainFact)]` macro
  - Depend on aura-journal for extensibility
  - Register in FactRegistry

### Commitment Tree (`commitment_tree/`)

**Purpose**: CRDT-based threshold key management and device membership.

**Modules**:
- `state.rs`: `TreeState` - full tree structure (branches, leaves, topology)
- `reduction.rs`: `reduce(OpLog<AttestedOp>) -> TreeState` - deterministic state derivation
- `application.rs`: `apply_verified()` - validate and apply operations
- `operations.rs`: `TreeOperationProcessor` - batch processing and queries
- `compaction.rs`: `compact()` - snapshot and GC
- `authority_state.rs`: Authority-internal device and guardian views

**Key Invariants** (docs/102_journal.md):
- OpLog is authoritative (immutable facts)
- Deterministic reduction (same OpLog → identical TreeState)
- Monotonic growth (append-only)
- Content-addressed (each fact identified by hash)

**Architecture Separation**:
- **aura-core** (Layer 1): Tree types + cryptographic commitment functions
- **aura-journal** (Layer 2): Tree state machine + CRDT semantics + validation

### CRDT Handlers (`crdt/`)

**Purpose**: Composable handler implementations for semilattice semantics.

**Handlers**:
- `CvHandler`: Join-semilattice (⊔) for accumulating state (counters, sets, logs)
- `MvHandler`: Meet-semilattice (⊓) for restricting permissions/policies
- `CmHandler`: Operation-based with causal ordering (collaborative editing)
- `DeltaHandler`: Incremental sync with fold threshold (bandwidth-constrained)

**Trait**: `CrdtHandler` - unified interface with `CrdtSemantics` introspection

### Algebraic Types (`algebra/`)

**Purpose**: Domain-specific CRDT types built on aura-core semilattice foundation.

**Types**:
- `AccountState`: Device membership (G-Set), monotonically growing
- `OpLog`: Operation log as OR-Set of attested operations
- `EpochLog`: Monotonic epoch counter and key rotation history
- `GuardianRegistry`: Guardian set membership
- `InvitationRecordRegistry`: Pending invitations with TTL
- `IntentPool`: Capability intent management
- Meet-lattice constraints: `DeviceCapability`, `ResourceQuota`, `SecurityPolicy`, `TimeWindow`

**Convergence Invariant** (docs/120_state_reduction.md):
All synchronization operations are idempotent and commutative.

### Journal API (`journal_api.rs`)

**Purpose**: Stable, high-level API hiding CRDT implementation details.

**Types**:
- `Journal`: Combines `AccountState`, `OpLog`, `FactJournal`
- `JournalFact`: Fact with attestation metadata
- `CommittedFact`: Persisted fact with CID
- `AccountSummary`: Read-only account state view

**Operations**:
- `new()`, `merge()`: Journal lifecycle
- `add_fact()`, `add_fact_ordered()`: Fact insertion
- `get_facts()`, `fact_count()`: Queries
- `derive_account_summary()`: State views

### Effects Implementation (`effects.rs`)

**Purpose**: Layer 2 application effect handler implementing `JournalEffects`.

**Handler**: `JournalHandler<C, S, A>`
- Composes `CryptoEffects`, `StorageEffects`, `BiscuitAuthorizationEffects`
- Persists journals via StorageEffects
- Derives context IDs for relational facts
- Integrates with `FactRegistry` for domain fact reduction

**Factory**: `JournalHandlerFactory` - creates configured handlers

## Inputs

- `aura-core`: Effect traits, domain types, semilattice traits, tree primitives, cryptographic utilities

## Outputs

- Fact types and journal operations
- Reduction engine: deterministic state derivation
- Extensibility infrastructure: `DomainFact`, `FactReducer`, `FactRegistry`
- CRDT handlers: `CvHandler`, `MvHandler`, `CmHandler`, `DeltaHandler`
- Commitment tree state machine
- `JournalHandler`: Application effect implementation

## Invariants

1. **Monotonic Growth**: `Journal_t+1 = Journal_t ⊔ δ` (facts only added, never removed)
2. **Deterministic Reduction**: Identical fact sets → identical states (all replicas)
3. **Immutability**: Facts are immutable once created; metadata updates are monotonic
4. **Namespace Isolation**: Authority journals and Context journals are distinct
5. **Content Addressing**: Facts identified by hash (CID)
6. **Convergence**: All semilattice operations commute and are idempotent

## Boundaries

**What aura-journal DOES**:
- Define fact semantics and validation rules
- Provide deterministic reduction logic
- Implement CRDT semilattice operations
- Define commitment tree state machine
- Implement `JournalEffects` application effect trait
- Provide extensibility for Layer 4/5 domain facts

**What aura-journal DOES NOT DO**:
- Storage implementations (use `JournalEffects` / `StorageEffects`)
- Multi-party protocol coordination (use `aura-protocol`)
- Runtime composition (use `aura-agent`)
- Handler registration (use `aura-agent/src/fact_registry.rs`)
- Direct OS access (time, filesystem, network - use effect traits)

## Key Design Patterns

### Two-Tier Fact System

**Protocol-Level Facts** (in `ProtocolRelationalFact`):
- Core protocol constructs with complex reduction logic
- Stay in `aura-journal` because they participate in cross-domain invariants
- Examples: GuardianBinding, RecoveryGrant, Consensus, AMP channel state

**Domain-Level Facts** (via `Generic` + `FactRegistry`):
- Application-specific facts from Layer 4/5 crates
- Use `DomainFact::to_generic()` for storage
- Reduced by registered `FactReducer` implementations
- Examples: ChatFact, InvitationFact, ContactFact, Home/Mute/Ban/Kick facts

### Layered Fact Patterns

See `docs/999_project_structure.md` §"Fact Implementation Patterns by Layer":

- **Layer 2 domain crates**: Use `aura_core::types::facts` (no aura-journal dependency)
- **Layer 4/5 feature crates**: Use `DomainFact` trait (register with FactRegistry)

This prevents Layer 2 → Layer 2 circular dependencies.

## Dependencies

- `aura-core`: Foundation types and traits
- External: `serde`, `async-trait`, `thiserror`, `uuid`

## Dependents

- Layer 3: `aura-effects` (handler implementations)
- Layer 4: `aura-protocol`, `aura-guards`, `aura-consensus`, `aura-amp`
- Layer 5: `aura-authentication`, `aura-chat`, `aura-invitation`, `aura-recovery`, `aura-relational`, `aura-social`
- Layer 6: `aura-agent`, `aura-simulator`, `aura-app`

## Documentation

- `docs/102_journal.md`: Fact-based journal specification
- `docs/101_accounts_and_commitment_tree.md`: Commitment tree semantics
- `docs/103_relational_contexts.md`: Context-scoped journals
- `docs/120_state_reduction.md`: Reduction and convergence
- `docs/999_project_structure.md`: Layer 2 architecture and fact patterns
