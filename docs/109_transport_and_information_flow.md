# Transport and Information Flow

This document describes the architecture of transport, guard chains, flow budgets, receipts, and information flow in Aura. It defines the secure channel abstraction and the enforcement mechanisms that regulate message transmission. It explains how context boundaries scope capabilities and budgets.

## 1. Transport Abstraction

Aura provides a transport layer that delivers encrypted messages between authorities. Each transport connection is represented as a `SecureChannel`. A secure channel binds a pair of authorities and a context identifier. A secure channel maintains isolation across contexts.

A secure channel exposes a send operation and a receive operation. The channel manages replay protection and handles connection teardown on epoch changes.

```rust
pub struct SecureChannel {
    pub context: ContextId,
    pub peer: AuthorityId,
    pub channel_id: Uuid,
}
```

This structure identifies a single secure channel. One channel exists per `(ContextId, peer)` pair. Channel metadata binds the channel to a specific context epoch.

## 2. Guard Chain

All transport sends pass through the guard chain defined in [Authorization](104_authorization.md). CapGuard evaluates Biscuit capabilities and sovereign policy. FlowGuard charges the per-context flow budget and produces a receipt. JournalCoupler records the accompanying facts atomically. Each stage must succeed before the next stage executes. Guard evaluation runs synchronously over a prepared `GuardSnapshot` and returns `EffectCommand` data. An async interpreter executes those commands so guards never perform I/O directly.

## 3. Flow Budget and Receipts

Flow budgets limit the amount of data that an authority may send within a context. The flow budget model defines a quota for each `(ContextId, peer)` pair. A reservation system protects against race conditions.

An authority must reserve budget before sending. A reservation locks a portion of the available budget. The actual charge occurs during the guard chain. If the guard chain succeeds, a receipt is created.

```rust
/// From aura-core/src/types/flow.rs
pub struct Receipt {
    pub ctx: ContextId,
    pub src: AuthorityId,
    pub dst: AuthorityId,
    pub epoch: Epoch,
    pub cost: u32,
    pub nonce: u64,
    pub prev: Hash32,
    pub sig: Vec<u8>,
}
```

This structure defines a receipt. A receipt binds a cost to a specific context and epoch. The sender signs the receipt. The `nonce` ensures uniqueness and the `prev` field chains receipts for auditing. The recipient verifies the signature. Receipts support accountability in multi-hop routing.

## 4. Information Flow Budgets

Information flow budgets define limits on metadata leakage. Budgets exist for external leakage, neighbor leakage, and group leakage. Each protocol message carries leakage annotations. These annotations specify the cost for each leakage dimension.

Leakage budgets determine if a message can be sent. If the leakage cost exceeds the remaining budget, the message is denied. Enforcement uses padding and batching strategies. Padding hides message size. Batching hides message frequency.

```rust
pub struct LeakageBudget {
    pub external: u32,
    pub neighbor: u32,
    pub in_group: u32,
}
```

This structure defines the leakage budget for a message. Leakage costs reduce the corresponding budget on successful send.

## 5. Context Integration

Capabilities and flow budgets are scoped to a `ContextId`. Each secure channel associates all guard decisions with its context. A capability is valid only for the context in which it was issued. A flow budget applies only within the same context.

Derived context keys bind communication identities to the current epoch. When the account epoch changes, all context identities must refresh. All secure channels for the context must be renegotiated.

```rust
pub struct ChannelContext {
    pub context: ContextId,
    pub epoch: u64,
    pub caps: Vec<Capability>,
}
```

This structure defines the active context state for a channel. All guard chain checks use these values.

## 6. Failure Modes and Observability

The guard chain defines three categories of failure. A denial failure occurs when capability requirements are not met. A block failure occurs when a flow budget check fails. A commit failure occurs when journal coupling fails.

Denial failures produce no observable behavior. Block failures also produce no observable behavior. Commit failures prevent sending and produce local error logs. None of these failures result in network traffic.

This design ensures that unauthorized or over-budget sends do not produce side channels.

## 7. Security Properties

Aura enforces no observable behavior without charge. A message cannot be sent unless flow budget is charged first. Capability gated sends ensure that each message satisfies authorization rules. Receipts provide accountability for multi-hop forwarding.

The network layer does not reveal authority structure. Context identifiers do not reveal membership. All metadata is scoped to individual relationships.

## 8. Secure Channel Lifecycle

Secure channels follow a lifecycle aligned with rendezvous and epoch semantics:

1. **Establishment**:
   - Ranch rendezvous per [Rendezvous Architecture](111_rendezvous.md) to exchange descriptors inside the [relational context](112_relational_contexts.md) journal.
   - Each descriptor contains transport hints, a handshake PSK derived from the context key, and a `punch_nonce`.
   - Once both parties receive offer/answer envelopes, they perform Noise IKpsk2 using the context-derived keys and establish a QUIC or relay-backed channel bound to `(ContextId, peer)`.

2. **Steady state**:
   - Guard chain enforces CapGuard → FlowGuard → JournalCoupler for every send.
   - FlowBudget receipts created on each hop are inserted into the [relational context](112_relational_contexts.md) journal so downstream peers can audit path compliance.

3. **Re-keying on epoch change**:
   - When the account or context epoch changes (as recorded in [Authority and Identity](102_authority_and_identity.md) / [Relational Contexts](112_relational_contexts.md)), the channel detects the mismatch, tears down the existing Noise session, and triggers rendezvous to derive fresh keys.
   - Existing receipts are marked invalid for the new epoch, preventing replay.

4. **Teardown**:
   - Channels close explicitly when contexts end or when FlowGuard hits the configured budget limit.
   - Receipts emitted during teardown propagate through the relational context journal so guardians or auditors can verify all hops charged their budgets up to the final packet.

By tying establishment and teardown to relational context journals, receipts become part of the same fact set tracked in `115_maintenance.md`, ensuring long-term accountability.

## 8. Privacy-by-Design Patterns

The `aura-transport` crate demonstrates privacy-by-design principles where privacy mechanisms are integrated into core types rather than added as external concerns. This section extracts key patterns from `aura-transport` for use across the codebase.

### 8.1 Core Principles

**Privacy-by-Design Integration**:
- Privacy mechanisms built into core types (Envelope, FrameHeader, TransportConfig)
- Privacy levels as first-class configuration, not bolt-on features
- Relationship scoping embedded in message routing
- Capability blinding at the envelope level

**Minimal Metadata Exposure**:
- Frame headers contain only essential routing information
- Capability hints are blinded before transmission
- Selection criteria hide detailed capability requirements
- Peer selection uses privacy-preserving scoring

**Context Isolation**:
- All messages scoped to RelationshipId or ContextId
- No cross-context message routing
- Connection state partitioned by context
- Context changes trigger re-keying

### 8.2 Privacy-Aware Envelope Usage

The `Envelope` type provides three privacy levels:

```rust
// Clear transmission (no privacy protection)
let envelope = Envelope::new(payload);

// Relationship-scoped transmission
let envelope = Envelope::new_scoped(
    payload,
    relationship_id,
    None, // Optional capability hint
);

// Fully blinded transmission
let envelope = Envelope::new_blinded(
    payload,
    blinded_metadata,
);
```

**Pattern**: Always use `new_scoped()` for relationship communication. Only use `new()` for public announcements. Use `new_blinded()` when metadata exposure must be minimized.

### 8.3 Privacy-Preserving Peer Selection

Peer selection must not reveal capability requirements or relationship structure:

```rust
let criteria = PrivacyAwareSelectionCriteria::for_relationship(relationship_id)
    .require_capability("threshold_signing") // Will be blinded
    .min_reliability(ReliabilityLevel::High)
    .prefer_privacy_features(true);

let selection = criteria.select_peers(&available_peers);
```

**Pattern**:
- Selection criteria blinded before network transmission
- Selection scores computed without revealing weights
- Rejected candidates not logged or exposed
- Selection reasons use generic categories, not specific capabilities

### 8.4 Common Privacy Pitfalls

**❌ Avoid**:
- Logging detailed capability requirements
- Exposing relationship membership in error messages
- Reusing connection state across contexts
- Sending capability names in clear text
- Correlating message sizes with content types

**✅ Do**:
- Use generic error messages ("authorization failed" not "missing capability: admin")
- Pad messages to fixed sizes when possible
- Rotate connection identifiers on epoch changes
- Blind capability hints before network transmission
- Use privacy-preserving selection scoring

### 8.5 Testing Privacy Properties

When testing transport code, verify:

1. **Context Isolation**: Messages sent in context A cannot be received in context B
2. **Metadata Minimization**: Only essential headers exposed, no capability details
3. **Selection Privacy**: Peer selection does not leak candidate set or ranking criteria
4. **Re-keying**: Context epoch changes trigger channel teardown and fresh establishment
5. **No Side Channels**: Timing, message size, error messages do not leak sensitive information

See `crates/aura-transport/src/types/tests.rs` and `crates/aura-transport/src/peers/tests.rs` for examples.

### 8.6 Integration with Guard Chain

Transport privacy integrates with the guard chain:

1. **CapGuard**: Evaluates blinded capability hints without exposing requirements
2. **FlowGuard**: Charges budget before transmission (no side channels on failure)
3. **LeakageTracker**: Accounts for metadata exposure in frame headers
4. **JournalCoupler**: Records minimal send facts (no capability details)

The combination ensures that:
- Unauthorized sends produce no network traffic
- Over-budget sends fail silently (no timing side channel)
- Metadata leakage tracked against context budget
- Fact commits reveal only necessary information

## 9. Sync Status and Delivery Tracking

Category A (optimistic) operations require UI feedback for sync and delivery status. Anti-entropy provides the underlying sync mechanism, but users need visibility into progress. See [Operation Categories](107_operation_categories.md) for the full consistency metadata type definitions.

### 9.1 Propagation Status

Propagation status tracks journal fact sync via anti-entropy:

```rust
use aura_core::domain::Propagation;

pub enum Propagation {
    /// Fact committed locally, not yet synced
    Local,

    /// Fact synced to some peers
    Syncing { peers_reached: u16, peers_known: u16 },

    /// Fact synced to all known peers
    Complete,

    /// Sync failed, will retry
    Failed { retry_at: PhysicalTime, retry_count: u32, error: String },
}
```

Anti-entropy provides callbacks via `SyncProgressEvent` to track progress:
- `Started` - sync session began
- `DigestExchanged` - digest comparison complete
- `Pulling` / `Pushing` - fact transfer in progress
- `PeerCompleted` - sync finished with one peer
- `AllCompleted` - all peers synced

### 9.2 Acknowledgment Protocol

For facts with `ack_tracked = true`, the transport layer implements the ack protocol:

**Transmission Envelope:**
```rust
pub struct FactEnvelope {
    pub fact: Fact,
    pub ack_requested: bool,  // Set when fact.ack_tracked = true
}
```

**FactAck Response:**
```rust
pub struct FactAck {
    pub fact_id: String,
    pub peer: AuthorityId,
    pub acked_at: PhysicalTime,
}
```

**Protocol Flow:**
1. Sender marks fact as `ack_tracked` when committing
2. Transport includes `ack_requested: true` in envelope
3. Receiver processes fact and sends `FactAck` response
4. Sender records ack in journal's ack table
5. `Acknowledgment` records are queryable for delivery status

### 9.3 Message Delivery Status

Message delivery derives from consistency metadata:

```rust
use aura_core::domain::{Propagation, Acknowledgment, OptimisticStatus};

// Delivery status is derived, not stored directly
let is_sending = matches!(status.propagation, Propagation::Local);
let is_sent = matches!(status.propagation, Propagation::Complete);
let is_delivered = status.acknowledgment
    .map(|ack| expected_peers.iter().all(|p| ack.contains(p)))
    .unwrap_or(false);
```

Status progression: `Sending (◐) → Sent (✓) → Delivered (✓✓) → Read (✓✓ blue)`

Read receipts are semantic (user viewed) and distinct from delivery (device received). They use `ChatFact::MessageRead` rather than the acknowledgment system.

### 9.4 UI Status Indicators

Status indicators provide user feedback:

```
Symbol  Meaning              Color    Animation
─────────────────────────────────────────────────
  ✓     Confirmed/Sent       Green    None
  ✓✓    Delivered            Green    None
  ✓✓    Read (blue)          Blue     None
  ◐     Syncing/Sending      Blue     Pulsing
  ◌     Pending              Gray     None
  ⚠     Unconfirmed          Yellow   None
  ✗     Failed               Red      None
```

For messages:
- Single checkmark (✓) = sent to at least one peer
- Double checkmark (✓✓) = delivered to recipient
- Blue double checkmark = recipient read the message

### 9.4 Integration with Operation Categories

Sync status applies to Category A operations only:
- Send message → delivery status tracking
- Create channel → sync status to context members
- Update topic → sync status to channel members
- Block contact → local only (no sync needed for privacy)

Category B and C operations have different confirmation models:
- Category B uses proposal/approval state
- Category C uses ceremony completion status

Lifecycle modes (A1/A2/A3) apply within these categories: A1/A2 updates are usable immediately but must be treated as provisional until A3 consensus finalization. Soft-safe A2 should publish convergence certificates and reversion facts so UI and transport can surface any reversion risk during the soft window.

See [Consensus - Operation Categories](106_consensus.md#17-operation-categories) for categorization details.

## 10. Anti-Entropy Sync Protocol

Anti-entropy implements journal synchronization between peers. The protocol exchanges digests, plans reconciliation, and transfers operations.

### 10.1 Sync Phases

1. **Load Local State**: Read local `Journal` (facts + caps) and the local operation log.
2. **Compute Digest**: Compute `JournalDigest` for local state.
3. **Digest Exchange**: Send local digest to peer and receive peer digest.
4. **Reconciliation Planning**: Compare digests and choose action (equal, LocalBehind, RemoteBehind, or Diverged).
5. **Operation Transfer**: Pull or push operations in batches.
6. **Merge + Persist**: Convert applied ops to journal delta, merge with local journal, persist once per round.

### 10.2 Digest Format

```rust
pub struct JournalDigest {
    pub operation_count: u64,
    pub last_epoch: Epoch,
    pub operation_hash: Hash32,
    pub fact_hash: Hash32,
    pub caps_hash: Hash32,
}
```

The `operation_count` is the number of operations in the local op log. The `last_epoch` is the max parent_epoch observed. The `operation_hash` is computed by streaming op fingerprints in deterministic order. The `fact_hash` and `caps_hash` use canonical serialization (DAG-CBOR) then hash.

### 10.3 Reconciliation Actions

| Digest Comparison | Action |
|-------------------|--------|
| Equal | No-op |
| LocalBehind | Request missing ops |
| RemoteBehind | Push ops |
| Diverged | Push + pull |

### 10.4 Retry Behavior

Anti-entropy can be retried according to `AntiEntropyConfig.retry_policy`. The default policy is exponential backoff with a bounded max attempt count.

### 10.5 Failure Semantics

Failures are reported with structured phase context:

- `SyncPhase::LoadLocalState`
- `SyncPhase::ComputeDigest`
- `SyncPhase::DigestExchange`
- `SyncPhase::PlanRequest`
- `SyncPhase::ReceiveOperations`
- `SyncPhase::MergeJournal`
- `SyncPhase::PersistJournal`

This makes failures attributable to a specific phase and peer.

## 11. Protocol Version Negotiation

All choreographic protocols participate in version negotiation during connection establishment.

### 11.1 Version Handshake Flow

```
Initiator                    Responder
   |                            |
   |-- VersionHandshakeRequest -->
   |     (version, min_version, capabilities, nonce)
   |                            |
   |<-- VersionHandshakeResponse -|
   |     (Accepted/Rejected)
   |                            |
[Use negotiated version or disconnect]
```

The handler is located at `aura-protocol/src/handlers/version_handshake.rs`.

### 11.2 Handshake Outcomes

| Outcome | Response Contents |
|---------|-------------------|
| Compatible | `negotiated_version` (min of both peers), shared `capabilities` |
| Incompatible | `reason`, peer version, optional `upgrade_url` |

### 11.3 Protocol Capabilities

| Capability | Min Version | Description |
|------------|-------------|-------------|
| `ceremony_supersession` | 1.0.0 | Ceremony replacement tracking |
| `version_handshake` | 1.0.0 | Protocol version negotiation |
| `fact_journal` | 1.0.0 | Fact-based journal sync |

## 12. Summary

The transport, guard chain, and information flow architecture enforces strict control over message transmission. Secure channels bind communication to contexts. Guard chains enforce authorization, budget, and journal updates. Flow budgets and receipts regulate data usage. Leakage budgets reduce metadata exposure. Privacy-by-design patterns ensure minimal metadata exposure and context isolation. All operations remain private to the context and reveal no structural information.

Sync status and delivery tracking provide user visibility into Category A operation propagation. Anti-entropy provides the underlying sync mechanism with digest-based reconciliation. Version negotiation ensures protocol compatibility across peers. Delivery receipts enable message read status for enhanced UX.
