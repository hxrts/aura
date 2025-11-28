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

All transport sends pass through the guard chain defined in [Authorization](109_authorization.md). CapGuard evaluates Biscuit capabilities and sovereign policy. FlowGuard charges the per-context flow budget and produces a receipt. JournalCoupler records the accompanying facts atomically. Each stage must succeed before the next stage executes. Guard evaluation runs synchronously over a prepared `GuardSnapshot` and returns `EffectCommand` data; an async interpreter executes those commands so guards never perform I/O directly.

## 3. Flow Budget and Receipts

Flow budgets limit the amount of data that an authority may send within a context. The flow budget model defines a quota for each `(ContextId, peer)` pair. A reservation system protects against race conditions.

An authority must reserve budget before sending. A reservation locks a portion of the available budget. The actual charge occurs during the guard chain. If the guard chain succeeds, a receipt is created.

```rust
pub struct Receipt {
    pub context: ContextId,
    pub from: AuthorityId,
    pub to: AuthorityId,
    pub epoch: u64,
    pub cost: u32,
    pub signature: Vec<u8>,
}
```

This structure defines a receipt. A receipt binds a cost to a specific context and epoch. The sender signs the receipt. The recipient verifies it. Receipts support accountability in multi-hop routing.

## 4. Information Flow Budgets

Information flow budgets define limits on metadata leakage. Budgets exist for external leakage, neighbor leakage, and group leakage. Each protocol message carries leakage annotations. These annotations specify the cost for each leakage dimension.

Leakage budgets determine if a message can be sent. If the leakage cost exceeds the remaining budget, the message is denied. Enforcement uses padding and batching strategies. Padding hides message size. Batching hides message frequency.

```rust
pub struct LeakageCost {
    pub external: u32,
    pub neighbor: u32,
    pub group: u32,
}
```

This structure defines the leakage cost of a message. Leakage costs reduce the corresponding budget on successful send.

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
   - Ranch rendezvous per [Rendezvous Architecture](110_rendezvous.md) to exchange descriptors inside the [relational context](103_relational_contexts.md) journal.
   - Each descriptor contains transport hints, a handshake PSK derived from the context key, and a `punch_nonce`.
   - Once both parties receive offer/answer envelopes, they perform Noise IKpsk2 using the context-derived keys and establish a QUIC or relay-backed channel bound to `(ContextId, peer)`.

2. **Steady state**:
   - Guard chain enforces CapGuard → FlowGuard → JournalCoupler for every send.
   - FlowBudget receipts created on each hop are inserted into the [relational context](103_relational_contexts.md) journal so downstream peers can audit path compliance.

3. **Re-keying on epoch change**:
   - When the account or context epoch changes (as recorded in `101_accounts_and_commitment_tree.md` / `103_relational_contexts.md`), the channel detects the mismatch, tears down the existing Noise session, and triggers rendezvous to derive fresh keys.
   - Existing receipts are marked invalid for the new epoch, preventing replay.

4. **Teardown**:
   - Channels close explicitly when contexts end or when FlowGuard hits the configured budget limit.
   - Receipts emitted during teardown propagate through the relational context journal so guardians or auditors can verify all hops charged their budgets up to the final packet.

By tying establishment and teardown to relational context journals, receipts become part of the same fact set tracked in `111_maintenance.md`, ensuring long-term accountability.

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

## 9. Summary

The transport, guard chain, and information flow architecture enforces strict control over message transmission. Secure channels bind communication to contexts. Guard chains enforce authorization, budget, and journal updates. Flow budgets and receipts regulate data usage. Leakage budgets reduce metadata exposure. Privacy-by-design patterns ensure minimal metadata exposure and context isolation. All operations remain private to the context and reveal no structural information.
