# Identifiers and Boundaries

This reference defines the identifiers that appear in Aura documents. Every other document should reuse these definitions instead of restating partial variants. Each identifier preserves structural privacy by design.

## 1. Authority Identifiers

### 1.1 AuthorityId

`AuthorityId` is a random UUID assigned to an authority journal namespace. It does not leak operator or membership metadata. All public keys, commitment trees, and attested operations reduce under this namespace. When a relationship references an account it uses the `AuthorityId` only.

```rust
pub struct AuthorityId(Uuid);
```

This identifier selects the journal namespace associated with the authority. The identifier does not encode structure or membership.

### 1.2 DeviceId

`DeviceId` is a random UUID identifying a device within a threshold account. Each device holds shares of the root key. The identifier is visible only inside the authority namespace and never crosses authority boundaries without explicit consent.

```rust
pub struct DeviceId(Uuid);
```

This identifier enables internal device management. External parties cannot enumerate devices from observing `AuthorityId` traffic.

### 1.3 LocalDeviceId

`LocalDeviceId` is an internal device identifier meaningful only within an authority. It uses a compact `u32` representation for efficiency. This identifier never appears in cross-authority communication.

```rust
pub struct LocalDeviceId(u32);
```

This compact representation optimizes internal journal operations. External observers see only `AuthorityId`.

### 1.4 GuardianId

`GuardianId` identifies a social recovery guardian. Guardians are trusted third parties that help recover account access. The identifier is a random UUID that does not reveal the guardian's own authority structure.

```rust
pub struct GuardianId(Uuid);
```

This identifier enables guardian configuration without exposing guardian identity relationships to network observers.

### 1.5 AccountId

`AccountId` is a legacy identifier being replaced by `AuthorityId`. Some code paths still reference it during the migration period. New code should use `AuthorityId` exclusively.

```rust
pub struct AccountId(Uuid);
```

This identifier exists for backward compatibility. It will be removed once migration completes.

## 2. Context Identifiers

### 2.1 ContextId

`ContextId` is a random UUID that identifies a `RelationalContext` or a derived subcontext. Context IDs are opaque on the wire. They only appear inside encrypted envelopes and receipts. Context IDs never encode participant lists or roles. All flow budgets, receipts, and leakage metrics scope to a `(ContextId, peer)` pair.

```rust
pub struct ContextId(Uuid);
```

This identifier enables cross-authority coordination without revealing relationship structure to observers.

### 2.2 SessionId

`SessionId` identifies an execution of a choreographic protocol. The identifier pairs a `ContextId` with a nonce. Session IDs are not long-lived. They expire when the protocol completes or when a timeout occurs. Protocol logs use `SessionId` to match receipts with specific choreographies.

```rust
pub struct SessionId(Uuid);
```

This identifier ensures protocol execution isolation. Different sessions within the same context remain distinguishable.

### 2.3 DkdContextId

`DkdContextId` identifies a Deterministic Key Derivation context. It combines an application label with a fingerprint to scope derived keys. This identifier enables privacy-preserving key derivation across application boundaries.

```rust
pub struct DkdContextId {
    app_label: String,
    fingerprint: [u8; 32],
}
```

This composite identifier enables application-scoped identity without cross-linking contexts.

## 3. Communication Identifiers

### 3.1 ChannelId

`ChannelId` identifies an AMP messaging substream scoped under a `RelationalContext`. It uses a `Hash32` representation. Channel IDs are opaque and do not reveal membership or topology.

```rust
pub struct ChannelId(Hash32);
```

This identifier enables multiplexed communication within a context. Observers cannot determine channel purpose from the identifier.

### 3.2 RelayId

`RelayId` identifies a pairwise communication context derived from X25519 keys. It forms the foundation for RID message contexts. The identifier is a 32-byte array derived from the shared secret.

```rust
pub struct RelayId([u8; 32]);
```

This identifier enables private pairwise communication. Observers cannot link relay traffic to authority identities.

### 3.3 GroupId

`GroupId` identifies a threshold group communication context. It derives from group membership and forms the foundation for GID message contexts. The identifier is a 32-byte array.

```rust
pub struct GroupId([u8; 32]);
```

This identifier enables group communication without revealing membership to observers.

### 3.4 MessageContext

`MessageContext` is an enum that unifies the three privacy context types. It enforces the privacy partition invariant by making contexts mutually exclusive.

```rust
pub enum MessageContext {
    Relay(RelayId),
    Group(GroupId),
    DkdContext(DkdContextId),
}
```

This type ensures messages route through exactly one privacy partition. Cross-partition routing requires explicit bridge operations.

### 3.5 ConnectionId

`ConnectionId` is a UUID identifying network connections with privacy-preserving properties. It does not encode endpoint information.

```rust
pub struct ConnectionId(Uuid);
```

This identifier enables connection management without leaking topology information.

## 4. Content Identifiers

### 4.1 ContentId

`ContentId` is a hash of canonical content bytes. It represents complete content such as files, documents, encrypted payloads, or CRDT state. The identifier contains a hash and optional size metadata. It does not reveal the author or recipient.

```rust
pub struct ContentId {
    hash: Hash32,
    size: Option<u64>,
}
```

Any party can verify payload integrity by hashing bytes and comparing with `ContentId`.

### 4.2 ChunkId

`ChunkId` identifies storage-layer chunks. Multiple chunks may comprise a single `ContentId`. The identifier contains a hash and optional sequence number.

```rust
pub struct ChunkId {
    hash: Hash32,
    sequence: Option<u32>,
}
```

This identifier enables content-addressable storage with deduplication. Observers cannot reconstruct content structure from chunk identifiers alone.

### 4.3 Hash32

`Hash32` is a raw 32-byte Blake3 cryptographic hash. It forms the foundation for content addressing throughout the system.

```rust
pub struct Hash32([u8; 32]);
```

This primitive supports all content-addressable operations. It provides collision resistance and preimage resistance.

### 4.4 DataId

`DataId` identifies stored data chunks in the Aura storage system. It uses a string representation with type prefixes such as `data:uuid` or `encrypted:uuid`.

```rust
pub struct DataId(String);
```

This identifier enables heterogeneous storage addressing. The prefix indicates storage layer requirements.

## 5. Journal Identifiers

### 5.1 FactId

`FactId` is a lightweight reference to facts in the journal. It uses a `u64` representation for efficient indexing and avoids cloning fact content.

```rust
pub struct FactId(u64);
```

This identifier enables efficient journal queries. It is internal to the journal layer and does not cross authority boundaries.

### 5.2 EventId

`EventId` uniquely identifies events within the effect API system. It uses a UUID representation.

```rust
pub struct EventId(Uuid);
```

This identifier enables event tracking and correlation. It appears in audit logs and debugging output.

### 5.3 OperationId

`OperationId` tracks operations across the system. The core version uses a UUID. The journal version uses an actor and sequence number for CRDT dependency tracking.

```rust
// Core version
pub struct OperationId(Uuid);

// Journal version for CRDT
pub struct OperationId {
    actor: ActorId,
    sequence: u64,
}
```

This identifier enables operation correlation and causal ordering.

## 6. Consensus Identifiers

### 6.1 ConsensusId

`ConsensusId` uniquely identifies a consensus instance. It derives from the prestate hash, operation hash, and nonce. Witnesses treat matching consensus identifiers as belonging to the same consensus instance.

```rust
pub struct ConsensusId(Hash32);
```

This identifier binds operations to prestates through hash commitment. See [Consensus](104_consensus.md) for protocol details.

### 6.2 FrostParticipantId

`FrostParticipantId` identifies a participant in threshold signing. It must be non-zero for FROST protocol compatibility.

```rust
pub struct FrostParticipantId(NonZeroU16);
```

This identifier enables threshold signature coordination. It is scoped to signing sessions and does not leak identity.

## 7. Social Topology Identifiers

### 7.1 BlockId

`BlockId` uniquely identifies a block in the urban social topology. Each user resides in exactly one block. The identifier is a 32-byte array.

```rust
pub struct BlockId([u8; 32]);
```

This identifier enables block membership management. See [Social Architecture](114_social_architecture.md) for the complete model.

### 7.2 NeighborhoodId

`NeighborhoodId` uniquely identifies a neighborhood. Neighborhoods are collections of blocks with adjacency relationships. The identifier is a 32-byte array.

```rust
pub struct NeighborhoodId([u8; 32]);
```

This identifier enables neighborhood governance and traversal policies.

## 8. Tree Identifiers

### 8.1 LeafId

`LeafId` uniquely identifies leaf nodes in the commitment tree. It remains stable across tree modifications and epoch rotations.

```rust
pub struct LeafId(u32);
```

This identifier enables stable references to tree leaves. See [Accounts and Commitment Tree](101_accounts_and_commitment_tree.md) for tree structure.

### 8.2 ProposalId

`ProposalId` identifies snapshot proposals. It wraps a hash to enable proposal deduplication and verification.

```rust
pub struct ProposalId(Hash32);
```

This identifier enables proposal tracking during tree operations.

## 9. Accountability Structures

### 9.1 Receipt

`Receipt` is the accountability record emitted by `FlowGuard`. Each receipt contains `ContextId`, source `AuthorityId`, destination `AuthorityId`, epoch, cost, nonce, and chained hash plus signature.

```rust
pub struct Receipt {
    pub context: ContextId,
    pub src: AuthorityId,
    pub dst: AuthorityId,
    pub epoch: Epoch,
    pub cost: u32,
    pub nonce: u64,
    pub prev: Hash32,
    pub sig: Signature,
}
```

Receipts prove that upstream participants charged their budget before forwarding. No receipt includes device identifiers or user handles. See [Transport and Information Flow](108_transport_and_information_flow.md) for receipt propagation.

## 10. Derived Keys

Aura derives per-context cryptographic keys from reduced account state and `ContextId`. Derived keys never surface on the wire. They only exist inside effect handlers to encrypt payloads, generate commitment tree secrets, or run DKD.

The derivation inputs never include device identifiers. Derived keys inherit the privacy guarantees of `AuthorityId` and `ContextId`. The derivation function uses `derive(account_root, app_id, context_label)` and is deterministic but irreversible.

## See Also

[Authority and Identity](100_authority_and_identity.md) describes the authority model in detail. [Relational Contexts](103_relational_contexts.md) covers cross-authority relationships. [Transport and Information Flow](108_transport_and_information_flow.md) documents receipt chains and flow budgets. [Social Architecture](114_social_architecture.md) defines blocks and neighborhoods.
