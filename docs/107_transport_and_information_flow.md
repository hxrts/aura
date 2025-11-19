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

All transport sends pass through the guard chain defined in [Authorization Pipeline](108_authorization_pipeline.md). CapGuard evaluates Biscuit capabilities and sovereign policy. FlowGuard charges the per-context flow budget and produces a receipt. JournalCoupler records the accompanying facts atomically. Each stage must succeed before the next stage executes.

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
   - Ranch rendezvous per `108_rendezvous.md` to exchange descriptors inside the relational context journal.
   - Each descriptor contains transport hints, a handshake PSK derived from the context key, and a `punch_nonce`.
   - Once both parties receive offer/answer envelopes, they perform Noise IKpsk2 using the context-derived keys and establish a QUIC or relay-backed channel bound to `(ContextId, peer)`.

2. **Steady state**:
   - Guard chain enforces CapGuard → FlowGuard → JournalCoupler for every send.
   - FlowBudget receipts created on each hop are inserted into the relational context journal so downstream peers can audit path compliance.

3. **Re-keying on epoch change**:
   - When the account or context epoch changes (as recorded in `101_accounts_and_ratchet_tree.md` / `103_relational_contexts.md`), the channel detects the mismatch, tears down the existing Noise session, and triggers rendezvous to derive fresh keys.
   - Existing receipts are marked invalid for the new epoch, preventing replay.

4. **Teardown**:
   - Channels close explicitly when contexts end or when FlowGuard hits the configured budget limit.
   - Receipts emitted during teardown propagate through the relational context journal so guardians or auditors can verify all hops charged their budgets up to the final packet.

By tying establishment and teardown to relational context journals, receipts become part of the same fact set tracked in `109_maintenance.md`, ensuring long-term accountability.

## 8. Summary

The transport, guard chain, and information flow architecture enforces strict control over message transmission. Secure channels bind communication to contexts. Guard chains enforce authorization, budget, and journal updates. Flow budgets and receipts regulate data usage. Leakage budgets reduce metadata exposure. All operations remain private to the context and reveal no structural information.
