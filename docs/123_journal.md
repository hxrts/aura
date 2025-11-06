# Aura Journal: Unified Threshold and Membership System

## Executive Summary

The Aura Journal is a unified CRDT-based graph structure that integrates threshold cryptography and group membership into a single data model. Rather than maintaining separate systems for "who can sign" (threshold) and "who's in the group" (membership tree), the Journal expresses threshold policies as structural properties of the graph itself.

A 2-of-3 threshold identity is not an algorithm with external configuration, it's a graph node with a `Threshold{2,3}` policy and three child device nodes. The structure *is* the security policy.

**Current Implementation**:
- Constructs own CRDTs from session types (MLS-type eventually consistent authenticated data structure)
- Uses `petgraph` for graph algorithms and cycle detection
- Uses external threshold-crypto libraries for share splitting
- Uses external capability token library for authorization
- Supports Group nodes for private messaging
- Implemented backend: Ed25519 + Blake3

---

## 1. Architecture and Design

### 1.1 Design Principles

The Journal unifies four traditionally separate systems into one CRDT graph:

| Traditional System | Journal Integration |
|-------------------|-------------------|
| **Membership Tree** | Graph nodes represent members (devices, identities, groups, guardians) |
| **Threshold Policy** | Node policies (All, Any, Threshold{m,n}) are structural properties |
| **Recovery** | Guardian subtrees implement social recovery natively |
| **Authorization** | Capability tokens bind OCAP tokens to journal resources |

This unified approach eliminates impedance mismatches:
- Membership changes and threshold updates are atomic CRDT operations
- No separate threshold coordination—policy is part of node definition
- Recovery is built into the graph structure, not an external ceremony
- Authorization uses capability tokens scoped to journal resources

### 1.2 Cryptographic Modularity

The Journal separates **structure** from **cryptographic realization**:

| Component | Dependency | Implementation |
|-----------|-----------|-----------------|
| **Node policy logic** (Threshold{m,n}, Contains edges) | None | Pure structural semantics |
| **CRDT merge rules** | None | Deterministic via Automerge |
| **Structural invariants** (acyclic, m-of-n semantics) | None | Graph validation |
| **Commitment derivation** | Hash function (Blake3) | Generic hash-based design |
| **Threshold share operations** | Threshold-crypto libraries | External FROST/SSS implementations |
| **AEAD wrapping of node secrets** | AES-GCM + KDF | Standard crypto primitives |
| **Capability token verification** | Signature scheme | External token library |

**Result**: 80-90% of the system (data structure, policies, CRDT merges, capability model, recovery logic) is **curve-agnostic**. Only threshold and key derivation layers depend on specific cryptographic backends.

#### Pluggable Crypto Architecture

**Current Implementation** (MVP):
- Single backend: Ed25519 with Blake3 hashing
- External threshold-crypto library for share operations
- Minimal versioning: backend identifiers (Ed25519V1, Blake3V1) prevent compatibility issues

**Extension Points** (for future phases):
- BLS12_381 for threshold signatures
- Alternative hash functions (SHA256, Poseidon)
- Post-quantum signature schemes
- Domain-specific optimizations (ZK-friendly hashes, hardware acceleration)

All backend changes remain isolated from the core Journal semantics—the graph structure and CRDT properties continue to work with any underlying cryptographic backend.

### 1.3 Tree Structure: K-Ary Threshold Policy Tree

The Journal uses a generic k-ary tree where every node carries a **policy** that determines how its secret can be unwrapped:

#### Node Types

Every node in the Journal is one of four kinds:
- **Device**: Leaf node with private key material
- **Identity**: Inner node representing an M-of-N threshold identity
- **Group**: Inner node with encrypted messaging key for private group communication
- **Guardian**: Leaf node representing a social recovery participant

#### Node Policies

Each node has a policy determining how its secret unwraps:
- **All**: All children must participate (AND logic)
- **Any**: Any one child can participate (OR logic)
- **Threshold{m,n}**: M-of-N children must participate

Policies are structural properties, not external configuration. When a policy is read from the graph, it defines the unwrapping semantics directly.

#### Graph Structure

Two edge types compose the graph:
- **Contains**: Participates in key derivation; must form an acyclic tree
- **GrantsCapability**: Binds capability tokens to resources; non-deriving

#### Commitment Derivation

Each node has a **commitment** computed as:
```
C(node) = H(
    tag = "NODE",
    kind,
    policy,
    epoch,
    sorted_child_commitments
)
```

This commitment:
- Is independent of the cryptographic backend (uses hash function)
- Is deterministic across replicas (children sorted by NodeId)
- Is stable for equality checks
- Suits zero-knowledge proofs and cross-domain verification

#### Threshold Unwrap/Wrap

When ≥m valid shares are present for a node:
1. Shares are verified against the node's commitment
2. The node secret is reconstructed using the threshold-crypto library
3. The reconstructed secret serves as the parent's key derivation input (KEK)
4. Parent secrets can then be unwrapped, enabling upward propagation

**Anti-replay**: Shares bind to `(node_id, epoch, policy_hash)`. Rotation increments epoch, invalidating old shares.

#### CRDT Friendliness

The graph structure merges deterministically via session-type-based consensus:
- Children are stored as sets (deterministically ordered during derivation)
- Concurrent add/remove merges via session-type agreement protocols
- Policy updates merge deterministically: for same `n`, higher `m` wins
- Share contributions accumulate; duplicates from same source replace previous entries
- Epoch-based share binding prevents replay after rotation

#### Local Views

Each device materializes only what it needs by starting at a chosen root node:
- Walks only Contains edges for key derivation
- References edges import commitments only (no secrets)
- Gives identities, groups, and guardians each an **eventually consistent** view

#### Guardian & Recovery Modeling

Recovery is modeled as a subtree:
- A **Recovery** node sits under an Identity root with `Threshold{g_m, g_n}`
- Guardian nodes are children under the recovery node
- Recovery = satisfying the recovery subtree's threshold, which rewraps up to identity

Guardians use capability tokens with:
- Resource scope: `journal://recovery/{node_id}/epoch/{epoch}`
- Time bounds: Standard token expiration fields prevent indefinite access
- Revocation: Standard token library revocation prevents compromised guardians

#### Safety Invariants

The Journal maintains these invariants:
1. **Acyclic Contains**: Contains edges form a DAG (enforced on operation application)
2. **Deterministic Order**: Children sorted by NodeId during derivation
3. **Share Binding**: Shares bind to `(node_id, epoch)` for anti-replay
4. **Epoch Increment**: Rotation increments epoch, invalidating old shares
5. **Forward Secrecy**: Old secrets become unrecoverable after rotation
6. **Post-Compromise Security**: New secrets generated with fresh randomness

---

## 2. Integration with Aura's Architecture

### 2.1 Architectural Layers

The Journal integrates seamlessly within Aura's clean architecture:

```
┌──────────────────────────────────────────────────┐
│ aura-agent (Layer 3: Device Runtime)             │
│ - Device-side flows (add_device, recover, etc.)  │
│ - Materializes IdentityView / GroupView          │
│ - Integrates capability token library            │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────┐
│ External Capability Library (Mature OCAP Impl)   │
│ - TokenIssuer/TokenVerifier                      │
│ - Resource scoping: journal://node/{id}          │
│ - Standard delegation, attenuation, revocation   │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────┐
│ aura-authentication (Layer 1: WHO Verification)  │
│ - Verifies capability token signatures           │
│ - No changes from Journal integration            │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────┐
│ aura-journal (Layer 0: Journal Implementation)   │
│ - Merklized threshold signature tree (CRDT)      │
│ - Automerge for deterministic CRDT operations    │
│ - Petgraph for graph algorithms                  │
│ - Threshold-crypto for share operations          │
│ - Handlers and middleware for composition        │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────┐
│ aura-types (Foundation Layer)                    │
│ - NodeId, NodeKind, NodePolicy type definitions  │
│ - EdgeId, EdgeKind definitions                   │
│ - Canonical types for all Journal primitives     │
└──────────────────────────────────────────────────┘
```

### 2.2 Integration with Existing Systems

#### aura-choreography (Choreographic Coordination)

The choreography infrastructure orchestrates distributed Journal operations:

**Share Contribution Protocol**: Coordinates M-of-N threshold operations
- Flexible agreement: M devices sufficient for most operations
- Split-brain resolution: Shares accumulate via session-type consensus; first group to reach M valid shares unlocks the secret
- Pattern: Initiator broadcasts intent → collect M shares → apply via choreographic witness

**Rotation Protocol**: Orchestrates node secret rotation
- Stricter requirements for epoch changes only: May want ⌊(N+M)/2⌋ + 1
- Conflicting rotations resolve deterministically: Higher epoch wins (session type ensures ordering)
- Pattern: Initiator broadcasts rotate intent → collect M+ acks → commit rotation

**Recovery Ceremony Protocol**: Coordinates guardian-assisted recovery
- Uses guardian subtree from Journal structure
- Guardians contribute shares per Journal policy
- Validates guardian capability tokens before share acceptance

#### aura-crypto (Cryptography Layer)

The cryptography layer provides building blocks; Journal composes them:
- **FROST & DKD**: Used for threshold signing and key derivation
- **HPKE**: Used for guardian share encryption
- **AES-GCM**: Used for node secret wrapping
- **Shamir SSS**: Used for share splitting via external libraries

No custom crypto code—all cryptographic operations use existing aura-crypto primitives and external verified libraries.

#### aura-store (Storage)

Store operations use keys derived from unwrapped Journal identity secrets. No changes to storage layer.

#### aura-transport (Communication)

Transport ships Journal operations as CRDT deltas. No changes required.

---

## 3. Data Model

### 3.1 Node Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeKind {
    /// Physical device with private key
    Device,
    /// Threshold identity (M-of-N devices)
    Identity,
    /// Private group with messaging capabilities
    Group,
    /// Guardian for social recovery
    Guardian,
}
```

### 3.2 Policies

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodePolicy {
    /// All children must participate (AND)
    All,
    /// Any one child can participate (OR)
    Any,
    /// M-of-N threshold requirement
    Threshold { m: u8, n: u8 },
}
```

All policies are **validated** on application:
- Threshold policies require `m ≤ n` and `m,n > 0`
- All/Any policies are always valid
- Invalid policies are rejected before node creation

### 3.3 Node Structure

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyNode {
    /// Unique node identifier (UUID-based)
    pub id: NodeId,
    /// Type of node (Device, Identity, Group, Guardian)
    pub kind: NodeKind,
    /// Policy for deriving/unwrapping this node's secret
    pub policy: NodePolicy,
    /// AEAD-encrypted node secret (wrapped with KEK)
    pub enc_secret: Vec<u8>,
    /// Per-child share metadata for threshold unwrap
    pub share_headers: Vec<ShareHeader>,
    /// AEAD-encrypted messaging key (Groups only)
    pub enc_messaging_key: Option<Vec<u8>>,
    /// Rotation counter (prevents replay attacks)
    pub epoch: u64,
    /// Cryptographic backend identifier (versioned)
    pub crypto_backend: CryptoBackendId,
    /// Hash function identifier (versioned)
    pub hash_function: HashFunctionId,
    /// Non-sensitive metadata (display name, timestamps, etc.)
    pub meta: BTreeMap<String, String>,
}
```

### 3.4 Edge Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeKind {
    /// Parent-child containment (acyclic, participates in derivation)
    Contains,
    /// OCAP binding (capability token → resource)
    GrantsCapability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEdge {
    /// Unique edge identifier
    pub id: EdgeId,
    /// Source node
    pub from: NodeId,
    /// Target node
    pub to: NodeId,
    /// Edge semantics
    pub kind: EdgeKind,
}
```

### 3.5 Example Structures

**Simple Identity (2-of-3)**:
```
Identity(Alice)[Threshold{2,3}]
  ├─ Device(Laptop)[leaf]
  ├─ Device(Phone)[leaf]
  └─ Device(Tablet)[leaf]
```

**Identity with Guardian Recovery**:
```
Identity(Alice)[Threshold{2,3}]
  ├─ Device(Laptop)[leaf]
  ├─ Device(Phone)[leaf]
  ├─ Device(Tablet)[leaf]
  └─ Recovery(Alice)[Threshold{2,3}]
       ├─ Guardian(Bob)[leaf]
       ├─ Guardian(Carol)[leaf]
       └─ Guardian(Dave)[leaf]
```

**Private Group with Messaging**:
```
Group(ProjectTeam)[Threshold{2,3}, messaging_key: encrypted]
  ├─ Member(Alice) ──(Contains)──► Identity(Alice)
  ├─ Member(Bob)   ──(Contains)──► Identity(Bob)
  └─ Member(Carol) ──(Contains)──► Identity(Carol)
```

---

## 4. Operations

The Journal supports these CRDT operations:

### 4.1 Node Operations

**AddNode**: Insert a new node into the Journal
- Idempotent: existing nodes merge by metadata
- Initializes with epoch=0 and empty share headers

**UpdateNodePolicy**: Change a node's threshold policy
- Triggers re-encryption with new key derivation
- Increments epoch (invalidates old shares)
- Propagates rewrap upward through parent nodes

**RotateNode**: Generate fresh secret and increment epoch
- Fresh randomness ensures forward secrecy
- Increments epoch (invalidates old shares)
- Generates new share headers for current policy
- Post-compromise secure: old secrets unrecoverable

### 4.2 Edge Operations

**AddEdge**: Establish relationship between nodes
- `Contains`: Creates derivation link (checked for acyclicity)
- `GrantsCapability`: Binds OCAP token to journal resource

**RemoveEdge**: Break relationship (soft delete with tombstone)
- Enables CRDT-friendly edge removal
- Contains removal triggers re-derivation

### 4.3 Threshold Operations

**ContributeShare**: Device contributes share toward unwrapping node
- Validated against node commitment
- Accumulated in Automerge structure
- When ≥m valid shares present, node unwraps automatically

### 4.4 Group Operations

**SendGroupMessage**: Encrypted message to group members
- Encrypted with unwrapped group messaging key
- Sender proves group membership via threshold participation

---

## 5. Security Properties

### 5.1 Topological Security

- **Acyclic Contains**: Enforced at operation application time
- **References Isolation**: No secrets cross References edges—only commitments
- **Deterministic Ordering**: Children sorted by NodeId for stable commitments

### 5.2 Cryptographic Security

- **Forward Secrecy**: Old secrets unrecoverable after rotation (epoch bump)
- **Post-Compromise Security**: New secrets independent of old material (fresh randomness)
- **Threshold Correctness**: <m shares fail, ≥m shares succeed (via threshold-crypto library)
- **Anti-Replay**: Shares bind to `(node_id, epoch, policy_hash)`

### 5.3 Authorization Security

- **OCAP-Gated**: All topology mutations require valid capability tokens
- **Revocation**: Revoked tokens prevent further operations
- **Delegation**: Delegated tokens carry subset of permissions

---

## 6. Usage Examples

### Creating an Identity with 3 Devices

```rust
// Create 3 device nodes
let laptop = KeyNode::new(device_id_1, NodeKind::Device, NodePolicy::Any);
let phone = KeyNode::new(device_id_2, NodeKind::Device, NodePolicy::Any);
let tablet = KeyNode::new(device_id_3, NodeKind::Device, NodePolicy::Any);

// Create identity with 2-of-3 threshold
let identity = KeyNode::new(
    identity_id,
    NodeKind::Identity,
    NodePolicy::Threshold { m: 2, n: 3 },
);

// Add to journal via CRDT
account_state.add_journal_node(laptop)?;
account_state.add_journal_node(phone)?;
account_state.add_journal_node(tablet)?;
account_state.add_journal_node(identity)?;

// Add derivation edges
account_state.add_journal_edge(KeyEdge::new(identity_id, device_id_1, EdgeKind::Contains))?;
account_state.add_journal_edge(KeyEdge::new(identity_id, device_id_2, EdgeKind::Contains))?;
account_state.add_journal_edge(KeyEdge::new(identity_id, device_id_3, EdgeKind::Contains))?;
```

### Adding a Guardian for Recovery

```rust
// Create guardian node
let guardian = KeyNode::new(guardian_id, NodeKind::Guardian, NodePolicy::Any);

// Create recovery subtree
let recovery = KeyNode::new(
    recovery_id,
    NodeKind::Identity,
    NodePolicy::Threshold { m: 2, n: 5 },
);

// Add nodes and edges
account_state.add_journal_node(guardian)?;
account_state.add_journal_node(recovery)?;
account_state.add_journal_edge(KeyEdge::new(identity_id, recovery_id, EdgeKind::Contains))?;
account_state.add_journal_edge(KeyEdge::new(recovery_id, guardian_id, EdgeKind::Contains))?;

// Bind capability token to recovery resource
let token = create_capability_token(
    "journal://recovery/{recovery_id}/epoch/1",
    guardian_public_key,
    expires_at,
);
account_state.grant_capability(token.id, recovery_id)?;
```

### Group Messaging

```rust
// Create group node with messaging key
let group = KeyNode::new(
    group_id,
    NodeKind::Group,
    NodePolicy::Threshold { m: 2, n: 3 },
);
group.enc_messaging_key = Some(wrapped_group_key);

// Add member references
for member_identity in members {
    account_state.add_journal_edge(
        KeyEdge::new(group_id, member_identity, EdgeKind::Contains),
    )?;
}

// Send encrypted message
account_state.send_group_message(group_id, encrypted_content, sender_proof)?;
```

---

## 7. Future Extensions

The Journal is designed for future enhancement:

### Phase 8+ Opportunities

1. **K-Ary Auto-Balancing**: Automatic tree rebalancing for large groups
2. **Alternative Curves**: BLS12_381, Pallas/Vesta, post-quantum signatures
3. **Composite Groups**: Group-in-group nesting for organization hierarchies
4. **Advanced Messaging**: Integration with additional group messaging protocols
5. **Optimization**: Incremental materialization and caching strategies
6. **Formal Verification**: Automated proofs of security properties

All future enhancements remain compatible with the core Journal structure and CRDT semantics.

---

## 9. Conclusion

The Journal is a unified CRDT-based graph that integrates threshold cryptography, membership, recovery, and authorization into a single coherent data model. By expressing **security as structure**, the Journal achieves:

- **Unified State Management**: Single source of truth for identity and cryptography
- **Deterministic Merging**: Session-type algebra guarantees convergence under all orderings (MLS-type eventually consistent authenticated data structure)
- **Composable Security**: Devices, guardians, and groups use same primitives
- **Future-Proof Design**: Cryptographic backends pluggable without structural changes

The implementation is complete and production-ready for Phase 1-7 operations.
