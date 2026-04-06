# Identifiers and Boundaries

This reference defines the identifiers that appear in Aura documents. Every other document should reuse these definitions instead of restating partial variants. Each identifier preserves structural privacy by design.

## 1. Authority Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `AuthorityId` | `Uuid` | Journal namespace for an authority. Does not leak operator or membership metadata. All public keys, commitment trees, and attested operations reduce under this namespace. Guardians are identified by their own `AuthorityId`. |
| `DeviceId` | `Uuid` | Device within a threshold account. Each device holds shares of the root key. Visible only inside the authority namespace. |
| `AccountId` | `Uuid` | Legacy identifier being replaced by `AuthorityId`. Exists for backward compatibility. |

## 2. Context Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `ContextId` | `Uuid` | Relational context or derived subcontext. Opaque on the wire, appears only inside encrypted envelopes and receipts. Never encodes participant lists or roles. Flow budgets and leakage metrics scope to `(ContextId, peer)` pairs. |
| `SessionId` | `Uuid` | Choreographic protocol execution instance. Pairs a `ContextId` with a nonce. Not long-lived. Expires when protocol completes or times out. |
| `DkdContextId` | `{ app_label: String, fingerprint: [u8; 32] }` | Deterministic Key Derivation context. Combines application label with fingerprint to scope derived keys across application boundaries. |

## 3. Communication Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `ChannelId` | `Hash32` | AMP messaging substream scoped under a relational context. Opaque. Does not reveal membership or topology. |
| `MessageId` | `Uuid` | Individual message within a channel. Scoped under a `ChannelId`. |
| `RelayId` | `[u8; 32]` | Pairwise communication context derived from X25519 keys. Foundation for RID message contexts. |
| `GroupId` | `[u8; 32]` | Threshold group communication context derived from group membership. Foundation for GID message contexts. |
| `MessageContext` | `enum { Relay, Group, DkdContext }` | Unifies the three privacy context types. Enforces mutual exclusivity. Cross-partition routing requires explicit bridge operations. |
| `ConnectionId` | `Uuid` | Network connection identifier with privacy-preserving properties. Does not encode endpoint information. |

## 4. Content Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `ContentId` | `{ hash: Hash32, size: Option<u64> }` | Hash of canonical content bytes (files, documents, encrypted payloads, CRDT state). Any party can verify integrity by hashing and comparing. |
| `ChunkId` | `{ hash: Hash32, sequence: Option<u32> }` | Storage-layer chunk identifier. Multiple chunks may comprise a single `ContentId`. Enables content-addressable storage with deduplication. |
| `Hash32` | `[u8; 32]` | Raw 32-byte BLAKE3 cryptographic hash. Foundation for all content addressing. Provides collision and preimage resistance. |
| `DataId` | `String` | Stored data chunk identifier with type prefixes (`data:uuid`, `encrypted:uuid`). Enables heterogeneous storage addressing. |

## 5. Journal Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `FactId` | `[u8; 32]` | Content-addressed fact identifier derived from fact hash. Enables deduplication and integrity verification. |
| `FactTypeId` | `String` | Registered fact type discriminator. Each domain crate declares its own type IDs for schema-aware encoding and reduction. |
| `FactOpId` | `[u8; 32]` | Attested operation identifier within the journal. |
| `EventId` | `Uuid` | Event identifier within the effect API system. Used in audit logs and debugging. |
| `OperationId` | `Uuid` | Operation tracking identifier. |

## 6. Consensus Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `ConsensusId` | `Hash32` | Consensus instance identifier derived from prestate hash, operation hash, and nonce. Binds operations to prestates through hash commitment. See [Consensus](108_consensus.md). |
| `FrostParticipantId` | `NonZeroU16` | Threshold signing participant. Must be non-zero for FROST protocol compatibility. Scoped to signing sessions. |

## 7. Social Topology Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `HomeId` | `Hash32` | Home in the social topology. Each user resides in exactly one home. See [Social Architecture](115_social_architecture.md). |
| `NeighborhoodId` | `Hash32` | Neighborhood (collection of homes with 1-hop link relationships). Enables governance and traversal policies. |

## 8. Tree Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `LeafId` | `u32` | Authority-internal leaf node in the commitment tree. Stable across tree modifications and epoch rotations. Used for internal topology, proofs, and key-rotation bookkeeping. See [Authority and Identity](102_authority_and_identity.md). |
| `ProposalId` | `Hash32` | Snapshot proposal identifier. Enables proposal deduplication and verification during tree operations. |

## 9. Ceremony and Recovery Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `CeremonyId` | `String` | Guardian ceremony identifier for key-rotation and enrollment flows. |
| `RecoveryId` | `String` | Recovery ceremony identifier for guardian-based recovery flows. |
| `InvitationId` | `String` | Invitation identifier for relationship and enrollment invites. |

## 10. Membership Identifiers

| Identifier | Type | Purpose |
|------------|------|---------|
| `MemberId` | `String` | Member identifier within groups or organizational structures. |
| `IndividualId` | `String` | Individual person or entity within the identity system. Can be derived from a `DeviceId` or DKD context. |
| `RelationshipId` | `[u8; 32]` | Identifies a specific relationship between two authorities. |

## 11. Accountability Structures

### Receipt

`Receipt` is the accountability record emitted by `FlowGuard`. It contains context, source authority, destination authority, epoch, cost, nonce, chained hash, and signature. Receipts prove that upstream participants charged their budget before forwarding. No receipt includes device identifiers or user handles.

Fields: `ctx: ContextId`, `src: AuthorityId`, `dst: AuthorityId`, `epoch: Epoch`, `cost: FlowCost`, `nonce: FlowNonce`, `prev: Hash32`, `sig: ReceiptSig`.

See [Transport and Information Flow](111_transport_and_information_flow.md) for receipt propagation.

## 12. Derived Keys

Aura derives per-context cryptographic keys from reduced account state and `ContextId`. Derived keys never surface on the wire. They only exist inside effect handlers to encrypt payloads, generate commitment tree secrets, or run DKD.

The derivation inputs never include device identifiers. Derived keys inherit the privacy guarantees of `AuthorityId` and `ContextId`. The derivation function uses `derive(account_root, app_id, context_label)` and is deterministic but irreversible.

## See Also

[Authority and Identity](102_authority_and_identity.md) describes the authority model in detail. [Relational Contexts](114_relational_contexts.md) covers cross-authority relationships. [Transport and Information Flow](111_transport_and_information_flow.md) documents receipt chains and flow budgets. [Social Architecture](115_social_architecture.md) defines homes and neighborhoods.
