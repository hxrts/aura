# Consistency Metadata Architecture

This document describes the unified architecture for tracking and communicating consistency status of database entries to application code and UI layers.

## Overview

Aura uses a unified consistency metadata system based on three orthogonal dimensions:

1. **Agreement (A1/A2/A3)**: How durable/finalized is this fact?
2. **Propagation**: Has this fact reached peers via gossip/sync?
3. **Acknowledgment**: Have specific peers confirmed receipt?

These dimensions are independent - a fact can be finalized but not yet propagated, or propagated but not finalized.

## Core Types

### Agreement Levels

Agreement indicates the finalization level of a fact, following the A1/A2/A3 taxonomy:

```rust
pub enum Agreement {
    /// A1: Provisional - usable immediately, may be superseded
    Provisional,

    /// A2: Soft-Safe - bounded divergence with convergence certificate
    SoftSafe { cert: Option<ConvergenceCert> },

    /// A3: Finalized - consensus-confirmed, durable, non-forkable
    Finalized { consensus_id: ConsensusId },
}
```

Key methods:
- `is_finalized()` - True if A3 finalized
- `is_safe()` - True if A2 or A3

### Propagation Status

Propagation tracks anti-entropy sync status:

```rust
pub enum Propagation {
    /// Only on this device
    Local,

    /// Sync in progress
    Syncing { peers_reached: u16, peers_known: u16 },

    /// Reached all known peers
    Complete,

    /// Sync failed, will retry
    Failed { retry_at: PhysicalTime, retry_count: u32, error: String },
}
```

Key methods:
- `is_complete()` - True if synced to all known peers
- `is_local()` - True if not yet synced
- `progress()` - Returns sync progress as 0.0 to 1.0

### Acknowledgment

Acknowledgment tracks explicit per-peer delivery confirmation:

```rust
pub struct Acknowledgment {
    pub acked_by: Vec<AckRecord>,
}

pub struct AckRecord {
    pub peer: AuthorityId,
    pub acked_at: PhysicalTime,
}
```

Key methods:
- `contains(peer)` - Check if a specific peer acked
- `count()` - Number of acks received
- `peers()` - Iterator over peers who acked

### Propagation vs Acknowledgment

| Aspect | Propagation | Acknowledgment |
|--------|-------------|----------------|
| What it tracks | Gossip sync reached peers | Peer explicitly confirmed receipt |
| How it's known | Transport layer observes | Requires ack protocol response |
| Granularity | Aggregate (count) | Per-peer with timestamp |
| Opt-in | Always available | Fact must request acks |
| Use case | "Is sync complete?" | "Did Alice receive this?" |

A fact can be:
- `Propagation::Complete` but `Acknowledgment` empty (synced but no ack protocol)
- `Propagation::Local` but `Acknowledgment` has entries (ack received before full sync)

## Category-Specific Status Types

Each operation category from [Operation Categories](117_operation_categories.md) has a purpose-built status type.

### Category A: OptimisticStatus

For optimistic operations with immediate local effect:

```rust
pub struct OptimisticStatus {
    pub agreement: Agreement,
    pub propagation: Propagation,
    pub acknowledgment: Option<Acknowledgment>,
}
```

**Use cases**: Send message, create channel, update profile, react to message

**UI patterns**:
```
◐  Sending      propagation == Local
✓  Sent         propagation == Complete
✓✓ Delivered    acknowledgment.count() >= expected.len()
◆  Finalized    agreement == Finalized
```

### Category B: DeferredStatus

For operations that require approval before taking effect:

```rust
pub struct DeferredStatus {
    pub proposal_id: ProposalId,
    pub state: ProposalState,
    pub approvals: ApprovalProgress,
    pub applied_agreement: Option<Agreement>,
    pub expires_at: PhysicalTime,
}

pub enum ProposalState {
    Pending,
    Approved,
    Rejected { reason: String, by: AuthorityId },
    Expired,
    Superseded { by: ProposalId },
}
```

**Use cases**: Change permissions, remove member, transfer ownership, archive channel

### Category C: CeremonyStatus

For blocking operations that must complete atomically:

```rust
pub struct CeremonyStatus {
    pub ceremony_id: CeremonyId,
    pub state: CeremonyState,
    pub responses: Vec<ParticipantResponse>,
    pub prestate_hash: Hash32,
    pub committed_agreement: Option<Agreement>,
}

pub enum CeremonyState {
    Preparing,
    PendingEpoch { pending_epoch: Epoch, required_responses: u16, received_responses: u16 },
    Committing,
    Committed { consensus_id: ConsensusId, committed_at: PhysicalTime },
    Aborted { reason: String, aborted_at: PhysicalTime },
    Superseded { by: CeremonyId, reason: SupersessionReason },
}
```

**Use cases**: Add contact, create group, guardian rotation, device enrollment, recovery

## Unified Consistency Type

For cross-category queries and generic handling:

```rust
pub struct Consistency {
    pub category: OperationCategory,
    pub agreement: Agreement,
    pub propagation: Propagation,
    pub acknowledgment: Option<Acknowledgment>,
}

pub enum OperationCategory {
    Optimistic,
    Deferred { proposal_id: ProposalId },
    Ceremony { ceremony_id: CeremonyId },
}
```

### ConsistencyMap

Query results include consistency metadata via `ConsistencyMap`:

```rust
pub struct ConsistencyMap {
    entries: HashMap<String, Consistency>,
}

impl ConsistencyMap {
    pub fn get(&self, id: &str) -> Option<&Consistency>;
    pub fn is_finalized(&self, id: &str) -> bool;
    pub fn acked_by(&self, id: &str) -> Option<&[AckRecord]>;
    pub fn propagation(&self, id: &str) -> Option<&Propagation>;
}
```

Usage with queries:

```rust
// Query with consistency metadata
let (messages, consistency) = handler.query_with_consistency(&MessagesQuery::default()).await?;

for msg in &messages {
    if consistency.is_finalized(&msg.id) {
        println!("{}: finalized", msg.content);
    } else {
        println!("{}: pending", msg.content);
    }
}
```

## Opt-In Acknowledgment Tracking

Facts opt into acknowledgment tracking at creation time:

```rust
// Create fact with ack tracking enabled
let fact = Fact::new(order, timestamp, content)
    .with_ack_tracking();

// Or via FactOptions
journal.append(fact, FactOptions { request_acks: true })?;
```

For ack-tracked facts, the transport layer automatically:
1. Includes `ack_requested: true` flag in transmission envelope
2. Receiving peers send `FactAck` responses upon processing
3. Ack responses are stored in journal's ack storage

## Delivery Policies

The app layer defines what "complete" means for each fact type via `DeliveryPolicy`:

```rust
pub trait DeliveryPolicy: Send + Sync {
    /// Who should acknowledge this fact?
    fn expected_peers(&self, fact: &Fact, context: &dyn PolicyContext) -> Vec<AuthorityId>;

    /// When should we stop tracking acks for this fact?
    fn should_drop_tracking(&self, consistency: &Consistency, expected: &[AuthorityId]) -> bool;
}
```

### Standard Policies

```rust
// Drop tracking once A3 finalized
pub struct DropWhenFinalized;

// Drop tracking once all expected peers acknowledged
pub struct DropWhenFullyAcked;

// Drop only when both finalized AND fully acked
pub struct DropWhenFinalizedAndFullyAcked;

// Drop when A2+ safe AND fully acked
pub struct DropWhenSafeAndFullyAcked;
```

### Policy Registration

```rust
let mut registry = PolicyRegistry::new();
registry.register::<MessageSentSealed>(Arc::new(DropWhenFullyAcked));
registry.register::<InvitationAccepted>(Arc::new(DropWhenFinalized));
registry.register::<GuardianBinding>(Arc::new(DropWhenFinalizedAndFullyAcked));
```

## Garbage Collection

Ack tracking storage is garbage collected based on delivery policies:

```rust
// GC based on policy evaluation
let result = ack_storage.gc_ack_tracking(&mut journal, |fact, consistency| {
    let policy = registry.get_policy(&fact);
    let expected = policy.expected_peers(&fact, &context);
    policy.should_drop_tracking(&consistency, &expected)
});

// Or simpler: GC based on consistency predicate
let result = ack_storage.gc_by_consistency(&mut journal, |c| c.agreement.is_finalized());
```

## Layer Responsibilities

| Concern | Layer | Implementation |
|---------|-------|----------------|
| Store fact content | Journal | Fact table |
| Track agreement level | Journal | `Agreement` field on Fact |
| Track propagation | Transport/Sync | `Propagation` updates |
| Store ack records | Journal | `AckStorage` |
| Define expected peers | App | `DeliveryPolicy::expected_peers()` |
| Define "complete" | App | `DeliveryPolicy::should_drop_tracking()` |
| Interpret status | App | Category-specific status types |
| Execute ack GC | Journal | Runs app policy |
| Display to user | UI | Consumes status types |

## Migration from Legacy Types

The following legacy types are deprecated:

| Legacy Type | Replacement | Location |
|-------------|-------------|----------|
| `SyncStatus` | `Propagation` | `aura_core::domain` |
| `DeliveryStatus` | `OptimisticStatus` | `aura_core::domain` |
| `ConfirmationStatus` | `DeferredStatus` | `aura_core::domain` |

Conversion methods are available:
- `SyncStatus::to_propagation()` / `From<SyncStatus> for Propagation`
- `DeliveryStatus::to_optimistic_status()` / `From<DeliveryStatus> for OptimisticStatus`
- `ConfirmationStatus::to_deferred_status(proposal_id)`

## Related Documentation

- [Operation Categories](117_operation_categories.md) - Category A/B/C definitions
- [Journal](102_journal.md) - Fact storage and consistency fields
- [Database Architecture](113_database.md) - Query system and isolation levels
- [Consensus](104_consensus.md) - A3 finalization via consensus
- [Transport and Information Flow](108_transport_and_information_flow.md) - Ack protocol
- [State Reduction Flows](120_state_reduction.md) - How consistency affects reduction
