# KeyFabric: Unified Threshold and Membership System

## Executive Summary

KeyFabric unifies Aura's threshold cryptography and group membership into a single CRDT-based graph structure. Instead of maintaining separate systems for "who can sign" (threshold) and "who's in the group" (membership tree), KeyFabric expresses **threshold policies as structural properties** of the graph itself.

**Core Insight**: A 2-of-3 threshold identity is not an algorithm with external configuration—it's a graph node with a `Threshold{2,3}` policy and three child device nodes. The structure **is** the rule.

**Implementation Strategy**:
- Use `automerge` for CRDT operations, `petgraph` for graph algorithms, `threshold-crypto`/`secret-sharing` for crypto
- External capability token library for authorization (no platform dependencies)
- Include Group nodes for private messaging from MVP start
- Single backend (Ed25519+Blake3) initially

**Result**: ~1,400 LOC implementation replacing ~2,800+ LOC = **Net -1,400 LOC reduction**

---

## 1. Conceptual Foundation

### 1.1 The Problem with Split Systems

Traditional architectures separate threshold logic from membership:

| Component | Traditional Approach | Issues |
|-----------|---------------------|---------|
| **Membership** | TreeKEM or MLS group tree | Rigid, requires coordination for changes |
| **Threshold** | Separate FROST/TSS state | Different data model, sync problems |
| **Recovery** | External ceremony | Not integrated with main system |
| **Authorization** | ACLs or separate RBAC | Another independent system |

This split creates **impedance mismatches**:
- Membership changes require coordinated TreeKEM updates
- Threshold operations need synchronous agreement
- Recovery is an out-of-band process
- Authorization logic lives in different data structures
- **Four separate systems** that must stay coordinated

### 1.2 KeyFabric's Unification

KeyFabric collapses these into **one CRDT graph**:

```
Graph Node = {
    identity: NodeId
    kind: Device | Identity | Group | Guardian
    policy: All | Any | Threshold{m,n}
    encrypted_secret: AEAD(node_secret, parent_KEK)
    share_headers: Vec<ShareCommitment>  // for threshold unwrap
    epoch: u64  // rotation counter
    messaging_key: Option<AEAD(group_messaging_key)>  // for Groups only
}

Graph Edge = {
    from: NodeId → to: NodeId
    kind: Contains | References | GrantsCapability
}
```

**Key Properties**:
1. **Structure = Security**: If graph topology is valid, cryptographic state is valid
2. **Policy as Topology**: Threshold policies are node properties, not external config
3. **CRDT-Native**: All operations merge deterministically via Automerge
4. **Composable**: Identity, group, recovery use same primitives

### 1.3 Cryptographic Modularity

**Critical Design Principle**: KeyFabric separates **structure** from **cryptographic realization**.

#### What's Curve-Agnostic vs. Curve-Dependent

| Component | Crypto Dependency | Portability Strategy |
|-----------|-------------------|---------------------|
| **Node policy logic** (Threshold{m,n}, Contains, References) | ❌ None | Pure structural semantics |
| **CRDT merge rules** | ❌ None | Deterministic merge of pure data ops |
| **Structural invariants** (acyclic, m-of-n) | ❌ None | Math-free constraints |
| **Commitment derivation** | ✅ Hash function | Generic `FabricHash` trait |
| **Threshold share generation/verification** | ✅ Curve + scheme | Trait-based backend (FROST/BLS/SSS) |
| **AEAD wrapping of node secrets** | ✅ Symmetric cipher + KDF | Abstracted symmetric backend |
| **Capability token signatures** | ✅ Signature scheme | Same or compatible backend |
| **Key derivation pipelines** (root → leaf) | ✅ KDF or curve-based DH | `CryptoBackend` trait |

**Result**: 80-90% of the system (data structure, policies, CRDT merges, capability model, recovery logic) is **curve-agnostic**. Only threshold proof and key derivation layers need cryptographic backends.

#### Pluggable Crypto Architecture

**MVP Simplification**: Ship with **Ed25519 + Blake3 only** for Phase 1-7. The abstraction exists but we defer multiple backend implementations.

**Future trait structure** (design documented, not implemented in MVP):

The system is designed to support pluggable backends through `CryptoBackend` and `FabricHash` traits that abstract over curves and hash functions. Key operations (hash_to_scalar, derive_commitment, combine_shares, sign_threshold, verify_signature, derive_key) are defined generically.

**MVP Implementation** (Phase 1-7):
- Single concrete backend: Ed25519 with Blake3 hashing
- No backend registry or selection logic
- No versioned backend enums (just use Ed25519V1 constant)
- Saves ~400 LOC of abstraction infrastructure

**Extension Point** (Phase 8):
When proven need arises (ZK-friendly curves, post-quantum migration), implement the full trait-based backend system with BLS12_381, Pallas/Vesta, or other curves

#### Where Crypto Really Matters

Only three layers must agree on cryptographic backend:

1. **Threshold Share Generation/Reconstruction**
   - FROST over Ed25519, BLS threshold signatures, etc.
   - Determines what "share" and "commitment" mean

2. **Commitment Hashing and Tree Roots**
   - Merkle or Poseidon root commitment
   - Defines cryptographic identity of fabric state
   - Parameterized: `FabricCommitment<H: FabricHash>`

3. **Capability Tokens**
   - Tokens carry threshold signatures
   - Verification uses same backend as fabric's key shares
   - Can separate OCAP signature scheme from fabric KDF if needed

Everything else (threshold policy logic, rotation epochs, CRDT operations, capability edges) remains **curve-agnostic**.

#### Crypto-Independent Invariants

These properties hold regardless of backend:

| Property | Backend-Independent | Explanation |
|----------|-------------------|-------------|
| Deterministic CRDT merges | ✅ | Pure state ops |
| Threshold m-of-n semantics | ✅ | Structure only |
| Rotation & epoch invalidation | ✅ | Epoch counter logic |
| Recovery subtrees | ✅ | Structure & policy only |
| Capability revocation | ✅ | CRDT operation |
| Derivation topology (Contains/References) | ✅ | Structural |
| Key wrapping (if AEAD abstracted) | ✅ | Backend parameterized |

#### Strategic Benefits

This decoupling provides:

1. **Cryptographic Upgrades**: Swap curves, hash functions, or AEAD schemes without touching identity structure or state logic
2. **Cross-Backend Compatibility**: Serialize, replay, and verify across backends with consistent commitment formats
3. **Domain-Specific Optimization**: Different curves per domain (Ed25519 on devices, BLS on servers, STARK-friendly hashes on ZK coprocessors)
4. **Future-Proofing**:
   - Migration to post-quantum signatures ✅
   - Use in ZK-proving circuits ✅
   - Multi-backend replication ✅

#### Design Rule of Thumb

> The KeyFabric defines **structure and semantics**, not cryptography.
> Cryptography is a **pluggable backend** that instantiates the structure's semantics.

### 1.4 Tree Structure: K-Ary Threshold Policy Tree

**Core Principle**: Use a single, generic **k-ary threshold policy tree** that scales from "3 devices + 5 guardians" to large groups while staying CRDT-friendly and efficient for key updates.

#### Single Node Type, Variable Arity

Every **inner node** carries a **policy**: `All | Any | Threshold{m,n}`.
Every **leaf** is a concrete participant (device, guardian) or a **Link** (reference to external subtree).

**Two edge kinds only**:
- `Contains` - Participates in key derivation; must form a tree (acyclic)
- `References` - Non-deriving reference to another root; used for "group includes identity"

Globally: **Forest of containment trees** inside one CRDT fabric. `References` edges make the overall graph a DAG, but derivation walks only `Contains`.

**Example Structure - Private Group with Messaging**:
```
Group(SecureTeam) [Threshold 2/3, messaging_key: encrypted]
  ├─(Contains) MemberRef(Alice) ──(References)──► Identity(Alice) [Threshold 2/3]
  ├─(Contains) MemberRef(Bob)   ──(References)──► Identity(Bob)   [Threshold 2/2]
  └─(Contains) MemberRef(Carol) ──(References)──► Identity(Carol) [Threshold 3/5 (guardians)]
```

**Private Messaging Properties**:
- Group node contains encrypted messaging key for message encryption/decryption
- Members derive group messaging capability from threshold participation
- Message encryption uses group's messaging key + member identity for forward secrecy
- Group rotation updates messaging key for post-compromise security

#### Tree Structure

**Simple fanout**: MVP uses flat children sets without complex balancing (deferred for groups).

#### Commitment Derivation (Merkle-ish but Policy-Aware)

Each node has a **commitment** with precise serialization for determinism:

**Commitment Formula**:
The commitment is computed as `H(tag || kind || policy_bytes || epoch || child_commitments)` where:
- `||` denotes concatenation
- `tag` is the UTF-8 bytes of "NODE"
- `kind` is serialized as a single byte (Device=0x01, Identity=0x02, Group=0x03, Guardian=0x04, Link=0x05)
- `policy_bytes` uses canonical encoding: All=0x01, Any=0x02, Threshold{m,n}=0x03||m||n
- `epoch` is little-endian u64
- `child_commitments` are sorted by NodeId bytes and concatenated
- Empty children (leaf nodes) contribute empty bytes

**Properties**:
- `Contains` children feed into commitment computation directly
- Root commitment is stable and deterministic across replicas

#### Threshold Unwrap/Wrap at Nodes

**Policy-at-node approach** (not external logic):
- Each inner node owns a **node secret** (AEAD-wrapped) and **share headers** for children
- **Two-phase threshold process**:
  1. **Agreement phase**: ⌊(N+M)/2⌋ + 1 devices must agree on current epoch (Byzantine majority)
  2. **Unwrap phase**: Once agreement reached, ≥M valid shares can unwrap the node secret
- This prevents split-brain while allowing threshold unwrap once consensus is established
- Start with **KEK + Shamir SSS** for simplicity and auditability
- Can swap to FROST/BLS later without structural changes

**Share Binding and Accumulation**:
Shares bind to `(node_id, epoch)` only - the policy is implicitly the one active at that epoch. Share accumulation is bounded to prevent memory exhaustion:
- Maximum 2 epochs retained (current and previous)
- Shares older than `current_epoch - 1` are automatically pruned
- Each share has a `valid_until_epoch` field (typically `epoch + 1`) for grace period during transitions
- Duplicate shares from same child for same epoch are replaced, not accumulated

#### CRDT Friendliness

**Why this structure merges cleanly**:
- Children of a node are a **set** (sorted deterministically at derivation)
- Concurrent add/remove merges via standard set CRDT rules
- Policy updates merge deterministically: for same `n`, higher `m` wins; otherwise, last-writer-wins with author ID tiebreak
- Stricter policy implies fresh rewrap (epoch bump)
- Shares bind to `(node_id, epoch)` to prevent replay after rotation

#### Local Views (Projection)

**Minimal materialized views**:
- Each replica materializes only what it needs by starting at a chosen **root**
- Walk only `Contains` edges for derivation
- When encountering a `Link` with `References`: import **commitment** of referenced root (no secret flow)
- Gives devices, identities, groups each an **eventually consistent** view with no duplication

#### Guardian & Recovery Modeling (Native)

**Recovery as a subtree**:
- Model a **Recovery** inner node under Identity root with `Threshold(g_m, g_n)`
- Guardians are children under recovery node
- Recovery = satisfying recovery subtree's threshold, which rewraps up to identity root

**Guardian Authorization via External Capability Library**:
Guardians use a mature external capability token library with no platform dependencies:
- Guardian capability tokens are standard capability tokens with resource scoping
- Resource format: `fabric://recovery/{node_id}/epoch/{epoch}` for epoch-specific access
- Tokens include standard `expires_at` field for time-bounding guardian authority
- Revocation happens through library-provided mechanisms (no custom code needed)
- This prevents guardians from contributing shares to arbitrary future epochs
- No dependency on external authorization platforms

**Example**:
```
Identity(Alice) [Threshold 2/3 devices]
  ├─(Contains) Device(Phone)
  ├─(Contains) Device(Laptop)
  ├─(Contains) Device(Desktop)
  └─(Contains) Recovery [Threshold 3/5 guardians]
       ├─(Contains) Guardian(Bob)
       ├─(Contains) Guardian(Carol)
       ├─(Contains) Guardian(Dave)
       ├─(Contains) Guardian(Eve)
       └─(Contains) Guardian(Frank)
```

#### Safety Invariants

**Core rules**:
1. **Acyclicity**: `Contains` subgraph must be acyclic
2. **Deterministic Order**: Children sorted by NodeId
3. **Share Binding**: Shares bind to `(node_id, epoch)`
4. **Epoch Increment**: Rotation increments epoch, invalidating old shares

---

## 2. Integration with Aura's Architecture

### 2.1 Architectural Layers

KeyFabric integrates seamlessly with Aura's clean architecture:

```
┌──────────────────────────────────────────────────┐
│ aura-agent (Layer 3)                             │
│ - Device runtime, local views                    │
│ - Materialize IdentityView / GroupView           │
│ - High-level flows (add_device, recover, etc.)   │
│ - Integrates external capability token library   │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────┐
│ External Capability Library (Layer 2)            │
│ - Mature capability token implementation         │
│ - TokenIssuer/TokenVerifier                      │
│ - Resource scoping: fabric://node/{id}/...       │
│ - Standard delegation, attenuation, revocation   │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────┐
│ aura-authentication (Layer 1)                    │
│ - WHO verification unchanged                     │
│ - Used to verify capability token signatures     │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────┐
│ aura-journal (Layer 0 - KeyFabric lives here)    │
│ - New module: journal::fabric                    │
│ - Uses Automerge native operations (no reducer)  │
│ - Uses petgraph for graph algorithms             │
│ - Uses threshold-crypto/secret-sharing libs      │
│ - Materialization engine                         │
│ - FabricEffects/Handler/Middleware pattern       │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────┐
│ aura-types (Layer -1)                            │
│ - Add: NodeId, NodeKind, NodePolicy              │
│ - Add: EdgeId, EdgeKind                          │
│ - Canonical types for fabric primitives          │
└──────────────────────────────────────────────────┘
```

### 2.2 Relationship to Existing Systems

#### **aura-protocol (Choreography Layer)**

**Choreographic Coordination for Distributed Fabric Operations**:

The choreography infrastructure in `aura-protocol` orchestrates multi-party fabric operations that require coordination across devices. Rather than each device independently applying CRDT operations, choreographed protocols ensure consistent distributed execution:

**Share Contribution Protocol** (New choreography):
- Coordinates M-of-N threshold operations across participating devices
- **Flexible agreement model**: M devices sufficient for most operations
- Split-brain scenarios (two groups contributing different shares) resolve via CRDT:
  - Shares accumulate from both groups
  - First group to reach M valid shares unlocks the secret
  - No conflict since shares are additive, not exclusive
- Choreography: Initiator broadcasts intent → collect M shares → apply to CRDT

**Fabric Rotation Protocol** (New choreography):
- Orchestrates node secret rotation across devices
- **Stricter requirement only for epoch changes**: May want ⌊(N+M)/2⌋ + 1 for safety
- Conflicting rotations resolve deterministically via CRDT merge rules:
  - Higher epoch always wins
  - Same epoch: last-writer-wins with author ID tiebreak
- Most rotations can proceed with just M participants
- Choreography: Initiator broadcasts rotate intent → collect M+ acks → commit rotation

**Recovery Ceremony Protocol** (Enhanced existing):
- `RecoveryProtocol` now uses guardian subtree from fabric
- Guardians contribute shares to recovery node per fabric policy
- Choreography validates guardian capability tokens before share acceptance
- Once threshold met, choreography coordinates identity rewrap

**Key Integration Points**:
- `DkdProtocol` reads identity secrets from unwrapped fabric nodes
- `ResharingProtocol` triggers `UpdateNodePolicy` operations atomically
- All protocols use fabric epochs for session versioning
- Choreography ensures CRDT operations apply in consistent order

**Benefits**:
- Type-safe coordination of distributed fabric operations
- Deadlock-free by construction (session types guarantee progress)
- Explicit protocol state machines prevent partial operations
- Choreography logs provide audit trail for threshold operations

#### **aura-crypto (Cryptography)**
- **Unchanged**: FROST, DKD, HPKE primitives
- **Integration**: KeyFabric uses crypto as building blocks
  - Node secrets → DKD inputs
  - Threshold policies → FROST signature requirements
  - KEK wrapping → HPKE or AES-GCM
  - Share splitting → Shamir Secret Sharing
- **Benefit**: Crypto layer stays pure, fabric layer composes it

#### **BeeKEM (in aura-journal/beekom.rs)**
- **Coexistence**: BeeKEM remains for MLS-style group communication
- **Distinction**:
  - **BeeKEM**: Convergent TreeKEM for efficient group messaging (MLS-compatible)
  - **KeyFabric**: Identity/authorization/threshold graph (Aura-native)
- **Integration**: Groups in KeyFabric can reference BeeKEM trees for communication
  - `KeyNode{kind: Group}` can have metadata pointing to BeeKEM tree ID
  - Fabric handles membership policy, BeeKEM handles message encryption
- **Benefit**: Best of both worlds - policy in fabric, efficient comms in BeeKEM

---

## 3. Data Model (Detailed)

### 3.1 Node Types

```rust
// In aura-types/src/fabric.rs

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodePolicy {
    /// All children must participate (AND)
    All,

    /// Any one child can participate (OR)
    Any,

    /// M-of-N threshold requirement
    Threshold { m: u8, n: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyNode {
    /// Unique node identifier (UUID)
    pub id: NodeId,

    /// Type of node (determines semantics)
    pub kind: NodeKind,

    /// Policy for deriving/unwrapping this node's secret
    pub policy: NodePolicy,

    /// AEAD-encrypted node secret (KEK-wrapped)
    /// Unwrapped when policy conditions met
    pub enc_secret: Vec<u8>,

    /// Per-child share metadata for threshold unwrap
    /// (index, commitment, etc.)
    pub share_headers: Vec<ShareHeader>,

    /// AEAD-encrypted messaging key (Groups only)
    /// Used for private group messaging encryption
    pub enc_messaging_key: Option<Vec<u8>>,

    /// Rotation counter (prevents replay attacks)
    pub epoch: u64,

    /// Cryptographic backend for this subtree (versioned enum)
    /// All descendants must use the same backend
    pub crypto_backend: CryptoBackendId,

    /// Hash function for commitment derivation (versioned enum)
    pub hash_function: HashFunctionId,

    /// Non-sensitive metadata (display name, created_at, etc.)
    pub meta: BTreeMap<String, String>,
}

/// Typed, versioned backend identifiers (no string comparison errors)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CryptoBackendId {
    Ed25519V1,
    BLS12_381V1,
    PallasVestaV1,
    Curve25519V1,
    // Future additions won't break existing nodes
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashFunctionId {
    Blake3V1,
    SHA256V1,
    PoseidonV1,
    // Future additions won't break existing nodes
}

impl KeyNode {
    /// Compute node commitment (Merkle-ish, policy-aware)
    ///
    /// C(node) = H(
    ///   tag = "NODE",
    ///   kind,
    ///   policy,
    ///   epoch,
    ///   children = sort_by_id([C(child_1), ..., C(child_k)])
    /// )
    ///
    /// This commitment is:
    /// - Independent of crypto backend specifics (uses hash_function)
    /// - Deterministic across replicas (sorted children)
    /// - Stable for equality checks
    /// - Suitable for ZK proofs and cross-domain verification
    pub fn compute_commitment<H: FabricHash>(
        &self,
        hash: &H,
        child_commitments: &[NodeCommitment],
    ) -> NodeCommitment {
        // Implementation deferred to Phase 2
        todo!("Implement commitment derivation per Section 1.4")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareHeader {
    /// Which child node this share is for
    pub child_id: NodeId,

    /// Share index (1..=n)
    pub index: u8,

    /// Commitment to the share (for verification)
    /// Serialized opaque bytes - interpretation depends on crypto_backend
    pub commitment: Vec<u8>,

    /// Public verification data
    /// Serialized opaque bytes - interpretation depends on crypto_backend
    pub proof: Vec<u8>,
}

/// Node commitment (32-byte hash)
/// Used for Merkle-ish tree structure with policy-aware derivation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCommitment(pub [u8; 32]);

impl NodeCommitment {
    /// Import commitment from References reference
    /// Used when a Link node references an external identity/group
    pub fn from_referenced_root(root_id: NodeId, fabric: &KeyFabric) -> Option<Self> {
        // Implementation deferred to Phase 2
        todo!("Import commitment from referenced root")
    }
}
```

### 3.2 Edge Types

```rust
/// Edge kinds define relationship semantics
///
/// Contains: Forms acyclic tree, participates in key derivation
/// References: Imports commitments only, no secret material
/// GrantsCapability: OCAP authorization binding
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    /// Parent-child containment (acyclic)
    /// Contributes to key derivation upward
    /// Example: Identity Contains Device, Identity Contains Guardian
    ///
    /// INVARIANT: Contains edges must form a DAG (enforced at apply time)
    Contains,

    /// OCAP binding (capability token → resource)
    /// Example: Guardian token GrantsCapability to Recovery subtree
    GrantsCapability,

    // References removed - deferred to Phase 8 with Group/Link support
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

### 3.3 Example Graph Structures (MVP Scope)

#### **Simple Identity (2-of-3)**
```
Identity(Alice)[Threshold{2,3}]
  ├─ Device(Laptop)[leaf]
  ├─ Device(Phone)[leaf]
  └─ Device(Tablet)[leaf]
```

#### **Identity with Guardian Recovery**
```
Identity(Alice)[Threshold{2,3}]
  ├─ Device(Laptop)[leaf]
  ├─ Device(Phone)[leaf]
  ├─ Device(Tablet)[leaf]
  └─ Recovery(Alice)[Threshold{2,3}]  // guardian subtree
       ├─ Guardian(Bob)[leaf]
       ├─ Guardian(Carol)[leaf]
       └─ Guardian(Dave)[leaf]
```

#### **Private Group with Messaging**
```
Group(ProjectTeam)[Threshold{2,3}, messaging_key: encrypted]
  ├─ MemberRef(Alice) ──(References)──► Identity(Alice)
  ├─ MemberRef(Bob)   ──(References)──► Identity(Bob)
  └─ MemberRef(Carol) ──(References)──► Identity(Carol)
```

**Private Messaging Flow**:
1. Group threshold met → unwrap group messaging key
2. Encrypt message with group key + forward secrecy nonce
3. All group members can decrypt using their threshold participation
4. Group rotation invalidates old messaging keys

---

## 4. Operation Set (CRDT)

### 4.1 Topology Operations

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FabricOp {
    /// Add a new node to the fabric
    AddNode {
        node: KeyNode,
    },

    /// Update a node's threshold policy
    /// Triggers rewrapping of encrypted secrets
    UpdateNodePolicy {
        node: NodeId,
        policy: NodePolicy,
    },

    /// Rotate a node's secret (increments epoch)
    /// Invalidates all previous shares
    RotateNode {
        node: NodeId,
        new_secret: Vec<u8>,  // encrypted with new KEK
        new_messaging_key: Option<Vec<u8>>,  // for Groups only
    },

    /// Add an edge between nodes
    AddEdge {
        edge: KeyEdge,
    },

    /// Remove an edge (soft delete with tombstone)
    RemoveEdge {
        edge: EdgeId,
    },

    /// Contribute a share for threshold unwrapping
    /// Multiple devices contribute to reach m-of-n
    ContributeShare {
        node: NodeId,
        child: NodeId,
        share_data: Vec<u8>,
        commitment: Vec<u8>,
        proof: Vec<u8>,
        epoch: u64,
    },

    /// Send encrypted message to group
    SendGroupMessage {
        group: NodeId,
        encrypted_content: Vec<u8>,
        sender_proof: Vec<u8>,  // proof of group membership
        epoch: u64,
    },

    /// Bind capability token to fabric resource
    GrantCapability {
        token_id: CapabilityId,
        target: ResourceRef,
    },

    /// Revoke capability token
    RevokeCapability {
        token_id: CapabilityId,
    },

    // === Operational Hooks (Deferred to Phase 8) ===
    //
    // BackupNodeSecret, MigrateDevice, EmergencyRotate, ThresholdEvent
    // are deferred to reduce MVP scope. These can be added later without
    // breaking changes to the core CRDT operations.
    //
    // For MVP: Use RotateNode for emergency rotation, regular logging for events
}
```

### 4.2 Operation Semantics

#### **AddNode**: Idempotent Insert
- If node exists with same ID, merge metadata
- Initialize with epoch=0
- No parent edges yet (added via AddEdge)

#### **UpdateNodePolicy**: Policy Change + Rewrap
- Change node's policy (e.g., 2-of-3 → 3-of-5)
- Generate new shares for new policy
- Increment epoch (invalidates old shares)
- Propagate rewrap upward in Contains hierarchy

#### **RotateNode**: Fresh Secret + Epoch Bump
- Generate new node secret
- Re-encrypt with new KEK
- Increment epoch
- Generate new shares for current policy
- **Post-compromise security**: Old secrets become unrecoverable

#### **AddEdge**: Establish Relationship
- `Contains`: Create parent-child derivation link (must be acyclic)
- `References`: Import public commitment (no secrets)
- `GrantsCapability`: Bind OCAP to resource

#### **RemoveEdge**: Break Relationship
- Soft delete with tombstone (CRDT-friendly)
- `Contains` removal triggers re-derivation
- `GrantsCapability` removal revokes access

#### **ContributeShare**: Threshold Participation
- Device contributes its share toward unwrapping parent node
- Validated against `(node_id, epoch, policy_hash, commitment)`
- When ≥m valid shares present, node secret unwraps
- **Anti-replay**: Shares bound to epoch

---

## 5. Security Invariants

### 5.1 Topological Invariants

1. **Acyclic Contains**: No derivation cycles
   - Enforced by reducer: reject operations creating cycles
   - Test: Property test with random Add/RemoveEdge sequences

2. **References Isolation**: No secrets cross References edges
   - Only public commitments imported
   - Test: Attempt to derive key through References path (should fail)

3. **Single Root per Tree**: Each derivation tree has one root node
   - Identity roots, Group roots
   - Test: Verify all paths lead to exactly one root

### 5.2 Cryptographic Invariants

1. **Forward Secrecy**: Old secrets unrecoverable after rotation
   - RotateNode increments epoch, old KEK unreachable
   - Test: Rotate, verify old shares don't unwrap

2. **Post-Compromise Security**: New secrets independent after rotation
   - Fresh randomness for each rotation
   - Test: Compromise old secret, verify can't predict new secret

3. **Threshold Correctness**: <m shares can't unwrap, ≥m can
   - Shamir SSS guarantees
   - Test: Property test with m-1, m, m+1 shares

4. **Share Anti-Replay**: Shares bound to (node, epoch, policy)
   - Old shares rejected after rotation
   - Test: Replay old share after RotateNode (should reject)

### 5.3 Authorization Invariants

1. **OCAP-Gated Mutations**: All topology changes require valid tokens
   - `UpdateNodePolicy` needs `Admin` permission on node
   - `ContributeShare` needs `ProtocolExecute` permission
   - Test: Attempt operations without token (should deny)

2. **Revocation Effectiveness**: Revoked tokens don't grant access
   - `RevokeCapability` prevents further operations
   - Test: Use revoked token (should deny)

3. **Delegation Attenuation**: Delegated tokens have subset of permissions
   - Already enforced by aura-authorization
   - Test: Delegate read-only, attempt write (should deny)

---

## 6. Code Deprecation & Removal Tasks

### 6.1 Deprecated Code Removal

**Goal**: Remove legacy systems replaced by KeyFabric to achieve the net LOC reduction

**Files to Remove** (~1,200 LOC):

#### **aura-authorization Crate** (Remove entire crate - ~400 LOC)
```bash
rm -rf crates/aura-authorization/
```
- `crates/aura-authorization/src/mod.rs`
- `crates/aura-authorization/src/policy.rs`
- `crates/aura-authorization/src/permissions.rs`
- `crates/aura-authorization/src/tokens.rs`
- `crates/aura-authorization/Cargo.toml`

#### **aura-policy Crate** (Remove entire crate - ~300 LOC)
```bash
rm -rf crates/aura-policy/
```
- `crates/aura-policy/src/mod.rs`
- `crates/aura-policy/src/rbac.rs`
- `crates/aura-policy/src/acl.rs`
- `crates/aura-policy/Cargo.toml`

#### **Legacy Membership Code** (~200 LOC)
- `crates/aura-journal/src/membership/tree.rs` - TreeKEM-style membership
- `crates/aura-journal/src/membership/coordinator.rs` - Centralized coordination
- `crates/aura-journal/src/membership/policy.rs` - External policy configuration
- `crates/aura-agent/src/membership/` - Entire directory (~150 LOC)

#### **Separate Threshold Systems** (~300 LOC)
- `crates/aura-crypto/src/threshold/coordinator.rs` - Threshold coordination logic
- `crates/aura-crypto/src/threshold/policy.rs` - External threshold policies
- `crates/aura-protocol/src/threshold/` - Separate threshold protocol directory
- `crates/aura-agent/src/threshold/` - Agent-level threshold management

#### **Redundant Recovery Code** (~200 LOC)
- `crates/aura-recovery/src/external_ceremony.rs` - Out-of-band recovery
- `crates/aura-recovery/src/guardian_coordination.rs` - Separate guardian system
- `crates/aura-agent/src/recovery/legacy.rs` - Legacy recovery flows

### 6.2 File Modifications (Remove imports/dependencies)

**Update Cargo.toml files**:
- `Cargo.toml` - Remove `aura-authorization`, `aura-policy` from workspace
- `crates/aura-agent/Cargo.toml` - Remove authorization/policy dependencies
- `crates/aura-journal/Cargo.toml` - Remove legacy membership dependencies
- `crates/aura-protocol/Cargo.toml` - Remove separate threshold dependencies

**Update import statements** (~100 LOC removals):
- `crates/aura-agent/src/lib.rs` - Remove `use aura_authorization::*`
- `crates/aura-agent/src/flows/` - Remove authorization imports across agent flows
- `crates/aura-journal/src/lib.rs` - Remove legacy membership imports
- `crates/aura-protocol/src/lib.rs` - Remove separate threshold imports

### 6.3 Test File Removals (~200 LOC)

**Authorization Tests**:
- `tests/authorization/` - Entire directory
- `crates/aura-authorization/tests/` - Entire directory
- `crates/aura-policy/tests/` - Entire directory

**Legacy Integration Tests**:
- `tests/integration/membership_tree.rs` - TreeKEM membership tests
- `tests/integration/threshold_coordination.rs` - Separate threshold tests
- `tests/integration/external_recovery.rs` - Out-of-band recovery tests

### 6.4 Documentation Cleanup

**Update references**:
- `README.md` - Remove mentions of deprecated crates
- `docs/architecture/overview.md` - Update architecture diagrams

### 6.5 Removal Checklist

**Phase 0.5: Pre-Implementation Cleanup** (1 week) ✅ COMPLETED
- [x] Remove `aura-authorization` crate entirely
- [x] Remove `aura-policy` crate entirely
- [x] Remove legacy membership code from `aura-journal`
- [x] Remove separate threshold systems from `aura-crypto`
- [x] Remove redundant recovery code from `aura-recovery`
- [x] Update all Cargo.toml dependencies
- [x] Remove deprecated imports across codebase
- [x] Remove obsolete test files
- [x] Update documentation references
- [x] Verify build succeeds after removals: `cargo build --workspace`

**Expected Result**: ~1,200 LOC removed before KeyFabric implementation begins

---

## 7. Implementation Plan (Revised)

### Phase 1: Foundation & Types (2-3 weeks)

**Goal**: Establish fabric types and CRDT operations without crypto

**Tasks**:
1. **Add external dependencies** (Cargo.toml):
   - `petgraph` for graph algorithms (cycle detection, traversal)
   - `threshold-crypto` or `secret-sharing` for threshold primitives
   - Capability token library (`biscuit-auth`, `caveat`, or `ucan`)
   - Existing: `automerge`, `blake3`, `ed25519-dalek`

2. **aura-types additions** (crates/aura-types/src/fabric.rs):
   - `NodeId`, `EdgeId`, `NodeKind`, `NodePolicy` types (~100 LOC)
   - `KeyNode`, `KeyEdge` structures
   - Simple concrete backend (Ed25519V1 constant, no trait)

3. **aura-journal::fabric module** (crates/aura-journal/src/fabric/) following effect/handler/middleware pattern:
   - `mod.rs` - Module root and re-exports (~50 LOC)
   - `types.rs` - Fabric-specific implementations (~100 LOC)
   - `ops.rs` - Map fabric operations to Automerge native ops (~100 LOC)
   - `graph.rs` - Petgraph integration for cycle detection (~50 LOC)
   - `effects.rs` - FabricEffects trait for external dependency injection (~50 LOC)
   - `handlers.rs` - Handler implementations for fabric operations (~100 LOC)
   - `middleware.rs` - FabricMiddleware stack for composable operation processing (~75 LOC)

4. **Integration with Automerge using Effects pattern**:
   - Use Automerge Map/Set operations directly (no custom reducer)
   - Add fabric state to `aura-journal::AccountState`
   - Leverage Automerge's built-in CRDT merge
   - Inject Automerge operations via FabricEffects trait for testability

5. **Basic Tests**:
   - Create/read nodes and edges via Automerge
   - Petgraph cycle detection integration
   - Automerge deterministic merge verification

**Acceptance Criteria**: ✅ COMPLETED
- [x] `cargo build -p aura-journal` succeeds
- [x] Can create graph with nodes and edges via Automerge operations
- [x] Petgraph cycle detection integration works
- [x] Concurrent Add/Remove operations merge via Automerge
- [x] Capability token library integration compiles
- [x] External dependencies reduce code by eliminating custom implementations
- [x] FabricEffects trait properly abstracts external dependencies for testing
- [x] FabricHandler and middleware stack follow existing aura patterns
- [x] Integration tests work with injected test effects

---

### Phase 2: Policy-Aware Derivation (3-4 weeks)

**Goal**: Implement key derivation along Contains edges (no threshold yet)

**Tasks**:
1. **Derivation Engine** (crates/aura-journal/src/fabric/derivation.rs ~150 LOC) using Effects pattern:
   - Use petgraph for graph traversal (leaves to root)
   - Compute commitment using Blake3: `H(tag="NODE", kind, policy, epoch, sorted_children)`
   - Simple fanout (no auto-bucketing for MVP - defer to Phase 8)
   - Children stored as Automerge Set with deterministic ordering
   - Inject graph operations and hashing via DerivationEffects trait
   - Use FabricHandler to orchestrate derivation middleware stack

2. **Materialization API** (crates/aura-journal/src/fabric/views.rs ~100 LOC) using Handler pattern:
   - `materialize_identity(root: NodeId) -> IdentityView`
   - Views contain commitments and topology via petgraph queries
   - No caching (naive recomputation for MVP simplicity)
   - Eventually consistent via Automerge's CRDT properties
   - Use ViewHandler with injected ViewEffects for graph queries and state access
   - Middleware for view composition and validation

3. **Integration with DKD** (~50 LOC):
   - Identity node secrets feed into existing `aura-crypto::dkd`
   - End-to-end: Fabric graph → Identity secret → App-specific key
   - Use Blake3 for all commitment derivation

**Deferred to Phase 8**:
- K-ary tree auto-balancing and bucketing
- References handling (no Group/Link nodes in MVP)
- Advanced caching strategies
- Complex topology optimizations

**Acceptance Criteria**: ✅ COMPLETED
- [x] Identity with 3 devices derives stable, deterministic commitment
- [x] DKD integration works end-to-end (fabric → identity secret → app key)
- [x] Petgraph traversal produces consistent ordering
- [x] Automerge Set operations maintain deterministic child ordering
- [x] Concurrent add/remove operations merge via Automerge CRDT

---

### Phase 3: Threshold Unwrapping (4-5 weeks)

**Goal**: Implement M-of-N threshold secret unwrapping

**Note**: This phase uses external threshold crypto libraries instead of custom implementations. No existing `aura-crypto` primitives are deprecated.

**Tasks**:
1. **Threshold Integration** (crates/aura-journal/src/fabric/threshold.rs ~200 LOC) following Effects/Handler pattern:
   - Use `threshold-crypto` or `secret-sharing` crate directly
   - Generate node secret and split via library functions
   - Encrypt node secret with reconstructed KEK: `enc_secret = AES-GCM(node_secret, KEK)`
   - Store minimal share headers (index + commitment from library)
   - Inject threshold operations via ThresholdEffects trait for testability
   - Use ThresholdHandler with middleware for validation, logging, and error handling

2. **Share Contribution** (~100 LOC) using Handler pattern:
   - Use library validation for share verification
   - Accumulate shares in Automerge Map structure
   - Library handles reconstruction when ≥m shares present
   - Simple upward propagation for parent rewrapping
   - ShareContributionHandler with middleware for validation, deduplication, and threshold checking

3. **Rotation Logic** (~50 LOC):
   - `RotateNode`: Fresh secret + epoch bump + library re-split
   - Automerge operations handle share invalidation
   - Epoch-based anti-replay via share binding

**Benefits of Library Usage**:
- Eliminates ~600 LOC of custom crypto bridge code
- Proven threshold implementations with security audits
- Standard interfaces reduce maintenance burden

**Acceptance Criteria**: ✅ COMPLETED
- [x] 2-of-3 identity unwraps with 2 shares, fails with 1
- [x] Rotation invalidates old shares
- [x] FS/PCS property tests pass
- [x] Threshold correctness property tests pass

---

### Phase 4: OCAP Integration & Recovery (3-4 weeks)

**Goal**: Gate operations with capability tokens and model social recovery

**Tasks**:
1. **Capability Token Integration** (crates/aura-journal/src/fabric/auth.rs ~100 LOC) using Effects/Handler/Middleware pattern:
   - Integrate external capability token library
   - Resource format: `fabric://node/{node_id}`, `fabric://recovery/{node_id}/epoch/{epoch}`
   - Use library's standard verification and delegation APIs
   - Gate fabric operations: `UpdateNodePolicy`, `ContributeShare`, etc.
   - Inject token operations via AuthEffects trait for testing and flexibility
   - AuthHandler with middleware for token validation, caching, and audit logging

2. **Recovery Subtree** (~100 LOC) using Handler pattern:
   - Model recovery as threshold subtree under identity
   - Guardian nodes with capability tokens for time-bounded access
   - Use external library for token expiration and revocation
   - Leverage existing threshold unwrap logic for guardian shares
   - RecoveryHandler with middleware for guardian validation, timing checks, and recovery orchestration

3. **Authorization Flow** (~50 LOC) using Middleware pattern:
   - Check capability tokens before all fabric mutations
   - Use library's resource scoping for fine-grained permissions
   - Standard delegation/attenuation via library APIs
   - AuthorizationMiddleware that wraps all fabric handlers with token validation

**Benefits of External Library**:
- Eliminates custom authorization infrastructure (~500 LOC saved)
- Mature, audited capability token implementation
- Standard patterns for delegation and revocation
- No external platform dependencies

**Acceptance Criteria**:
- [ ] All mutations require valid capability tokens from external library
- [ ] Guardian recovery flow works end-to-end
- [ ] Revoked guardian tokens prevent contribution (library handles)
- [ ] Resource scoping works: `fabric://node/{id}`, `fabric://recovery/{id}/epoch/{epoch}`

---

### Phase 5: Agent Flows & APIs (2-3 weeks)

**Goal**: High-level agent APIs for common operations

**Tasks**:
1. **aura-agent flows** (crates/aura-agent/src/fabric/) using Agent Handler pattern consistent with existing flows:
   - `add_device(identity_root, device_pub_key) -> NodeId`
   - `remove_device(identity_root, device_id)`
   - `grant_guardian(identity_root, guardian_id, capability_token)`
   - `revoke_guardian(identity_root, guardian_id)`
   - `create_group(member_identities, policy) -> NodeId`
   - `join_group(group_root, identity_root)`
   - `leave_group(group_root, identity_root)`
   - `send_group_message(group_root, content) -> MessageId`
   - `decrypt_group_message(group_root, encrypted_message) -> Vec<u8>`
   - `rotate_identity(identity_root)`
   - `rotate_group(group_root)`
   - Each flow uses AgentHandler with FabricEffects injection for journal/crypto operations
   - Middleware stack for validation, logging, and error handling consistent with existing agent patterns

2. **Local Views** using reactive Effects pattern:
   - `IdentityView`: devices, recovery guardians, derived secrets
   - `GroupView`: member identities, group messaging key, message history
   - `MessagingView`: encrypted group messages, decryption capabilities
   - Reactive updates when journal changes via ViewEffects trait
   - ViewHandler with change notification middleware for real-time updates

3. **CLI Integration** (crates/aura-cli/):
   - `aura fabric list-devices`
   - `aura fabric add-device <name>`
   - `aura fabric list-guardians`
   - `aura fabric add-guardian <identity>`
   - `aura fabric create-group <name> <members>`
   - `aura fabric send-message <group> <content>`
   - `aura fabric list-messages <group>`
   - `aura fabric recover`

**Acceptance Criteria**:
- [ ] Can add/remove devices via agent API
- [ ] Can add/revoke guardians via agent API
- [ ] Can create/join/leave groups with private messaging
- [ ] Can send/receive encrypted group messages
- [ ] Message encryption uses group threshold policies
- [ ] Views update reactively with journal changes
- [ ] CLI commands work end-to-end including messaging

---

### Phase 6: Concurrency & Properties (2-3 weeks)

**Goal**: Ensure correctness under concurrent operations

**Tasks**:
1. **Property Tests** (crates/aura-journal/src/fabric/tests/properties.rs):
   - **Convergence**: Shuffle operation order, verify same final state
   - **Liveness**: If ≥m shares arrive, unwrap eventually happens
   - **Isolation**: Secrets never leak across References edges
   - **Acyclicity**: Random Add/RemoveEdge never creates cycles

2. **Fuzz Testing** (fuzz/fabric_ops.rs):
   - Generate random operation streams
   - Apply to multiple replicas with different orderings
   - Verify deterministic convergence
   - Check for panics or invariant violations

3. **Concurrency Scenarios**:
   - Concurrent `UpdateNodePolicy` from different devices
   - Concurrent `RotateNode` on same identity
   - Concurrent `AddEdge` / `RemoveEdge` on same nodes
   - Concurrent `ContributeShare` reaching threshold

**Acceptance Criteria**:
- [ ] All property tests pass with 1000+ iterations
- [ ] Fuzz tests run for 1M operations without panics
- [ ] Concurrent operations documented and tested
- [ ] Convergence proofs hold under all scenarios

---

### Phase 7: Performance & Ergonomics (2-3 weeks)

**Goal**: Optimize and polish for production use

**Tasks**:
1. **Incremental Materialization**:
   - Cache derived views per root node
   - Invalidate cache only for affected subtrees
   - Change notifications for reactive UIs

2. **Operation Compaction**:
   - Garbage collect superseded shares (old epochs)
   - Compact tombstones for RemoveEdge operations
   - Maintain CRDT causality while reducing history

3. **WASM Bindings** (crates/aura-wasm/):
   - Expose fabric operations to JS/TS
   - WebWorker for background CRDT merge
   - Example: Multi-device web app with offline sync

4. **Documentation**:
   - Architecture guide (docs/architecture/keyfabric.md)
   - API reference (generated docs)
   - Tutorial: Building a multi-device app
   - Security model whitepaper

**Acceptance Criteria**:
- [ ] Memory usage stable with growing history
- [ ] Sub-millisecond materialization for typical graphs
- [ ] WASM examples work in browser
- [ ] Documentation complete and reviewed

---

## 8. Testing Strategy

### 8.1 Unit Tests

**Per-Module Coverage**:
- `fabric/types.rs`: Serialization, NodePolicy validation
- `fabric/ops.rs`: Operation encoding/decoding
- `fabric/handlers.rs`: Handler implementations with mock effects
- `fabric/middleware.rs`: Middleware composition and error handling
- `fabric/effects.rs`: Effects trait implementations and test doubles
- `fabric/graph.rs`: Cycle detection, traversal algorithms
- `fabric/threshold.rs`: KEK derivation, share split/combine
- `fabric/auth.rs`: OCAP token validation

### 8.2 Integration Tests

**Cross-Module Scenarios** (tests/fabric/) using test effects and mock handlers:
1. `fabric_projection.rs`: Create graph, materialize views, verify commitments
2. `threshold_unwrap.rs`: Contribute shares, verify unwrap at threshold
3. `guardian_recovery.rs`: Lose devices, guardians reconstruct, new device joins
4. `refers_to_isolation.rs`: Group can't access identity secrets
5. `concurrent_updates.rs`: Interleaved operations converge deterministically
6. `fs_pcs_properties.rs`: Forward secrecy and post-compromise security
7. `ocap_gating.rs`: Operations without tokens denied
8. `effects_injection.rs`: Verify all external dependencies properly injected via effects
9. `middleware_composition.rs`: Test middleware stack behavior and error propagation

### 8.3 Property Tests

**Invariants** (via proptest):
- **Convergence**: `∀ op_sequence, ∀ orderings → same final state`
- **Acyclicity**: `∀ graph operations → no Contains cycles`
- **Threshold**: `∀ node with Threshold{m,n} → <m shares fail, ≥m succeed`
- **Isolation**: `∀ References edges → no secrets traverse`
- **Anti-Replay**: `∀ shares with old epoch → rejected`

### 8.4 Fuzz Tests

**Random Operation Streams**:
- Generate 1M random FabricOp operations
- Apply to multiple replicas with permuted orderings
- Check for: panics, deadlocks, state divergence, invariant violations
- Run continuously in CI

---

## 9. Integration Notes

### 9.1 With Existing Crates

#### **aura-protocol**
- **Status**: Minimal changes
- **Integration**:
  - DKD protocol uses `materialize_identity()` to get secrets
  - Resharing protocol triggers `UpdateNodePolicy()` operations
  - Recovery protocol uses guardian subtree for reconstruction
- **Benefit**: Session types validate fabric operations

#### **aura-authentication**
- **Status**: No changes
- **Usage**: Verify signatures on capability tokens that gate fabric operations

#### **External Capability Token Library**
- **Status**: New integration (replaces aura-authorization/aura-policy)
- **Integration**:
  - Use mature capability token library (recommendation: `biscuit-auth`)
  - Resource scoping: `fabric://node/{node_id}`, `fabric://tree/{root_id}`
  - Standard token delegation, attenuation, and revocation
  - No custom authorization code needed
- **Benefits**: Removes ~500 LOC of custom auth code, no external platform dependencies

#### **aura-crypto**
- **Status**: No changes
- **Usage**: KeyFabric uses FROST, DKD, HPKE as building blocks

#### **aura-store**
- **Status**: No changes
- **Integration**: Store chunks using keys derived from fabric identity secrets

#### **aura-transport**
- **Status**: No changes
- **Integration**: Ship fabric operations as journal deltas

### 9.2 With BeeKEM

**Coexistence Strategy**:
- **BeeKEM**: Efficient group messaging (MLS-compatible)
- **KeyFabric**: Identity and authorization graph (Aura-native)

**Integration**:
- Groups in KeyFabric have metadata pointing to BeeKEM tree IDs
- Fabric manages membership policy (who can join/leave)
- BeeKEM handles message encryption (efficient ratcheting)
- Best of both worlds: policy in fabric, comms in BeeKEM

**Migration Path**:
- Phase 1-7: Build KeyFabric alongside BeeKEM
- Future: Optionally deprecate BeeKEM for simpler deployments
- BeeKEM remains for MLS interoperability requirements

---

## 10. Security Considerations

### 10.1 Threat Model

**Assumptions**:
- Adversary can compromise ≤m-1 devices
- Adversary observes all network traffic
- Adversary can arbitrarily delay/reorder messages
- Adversary cannot break AES-GCM, Ed25519, Shamir SSS

**Goals**:
- **Confidentiality**: Secrets unrecoverable with <m shares
- **Integrity**: Invalid operations rejected by all replicas
- **Availability**: System operational with ≥m honest devices
- **Forward Secrecy**: Old secrets unrecoverable after rotation
- **Post-Compromise Security**: New secrets independent after rotation

### 10.2 Attack Scenarios

| Attack | Mitigation |
|--------|-----------|
| **Replay old shares** | Epoch binding: shares include epoch, old ones rejected |
| **Forge share commitments** | Cryptographic commitment scheme (Pedersen or similar) |
| **Create derivation cycle** | Cycle detection in reducer, reject cyclic AddEdge |
| **Leak secrets via References** | Type-level separation: References can't carry secrets |
| **Bypass OCAP** | All operations gated by policy layer |
| **Compromise <m devices** | Insufficient shares, cannot unwrap |
| **Compromise ≥m devices** | Rotation provides post-compromise security |

### 10.3 Cryptographic Primitives

**For Phase 3 (Initial)**:
- **KEK Derivation**: HKDF from node secret
- **Secret Sharing**: Shamir Secret Sharing (SSS)
- **Encryption**: AES-GCM for AEAD
- **Commitments**: Pedersen commitments for share verification

---

## 11. Timeline & Success Criteria

**MVP Phases (15-20 weeks)**:
0.5. Pre-Implementation Cleanup (1 week)
1. Foundation & Types (2 weeks)
2. Policy-Aware Derivation (2-3 weeks)
3. Threshold Unwrapping (3-4 weeks)
4. Capability Integration & Recovery (2-3 weeks)
5. Agent Flows & APIs (2 weeks)
6. Concurrency & Properties (2 weeks)
7. Performance & Ergonomics (1-2 weeks)

**Success Criteria**:

- **Functional**: Multi-device identities with threshold policies, guardian recovery, secure device add/remove, private group messaging
- **Security**: Forward secrecy, post-compromise security, threshold correctness verified, message confidentiality
- **Testing**: Property tests, integration tests, basic fuzz testing, messaging security tests
- **Performance**: Sub-millisecond materialization, stable memory usage, efficient message encryption
- **Architecture**: Net -1,400 LOC reduction using external libraries

---

## 12. Conclusion

KeyFabric unifies threshold cryptography and membership into a single CRDT-based graph structure, reducing complexity from four separate systems to one. The structure **is** the security policy.

**Result**: A system where **identity is relational**, **security is structural**, and **trust is composable**.
