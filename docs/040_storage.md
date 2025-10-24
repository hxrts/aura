# 040 · Unified Storage Specification (Phase 1)

**Status:** Updated for Keyhive, SBB Integration & Session Types
**Version:** 3.1
**Created:** Original MVP + October 2025 Integration Update + Session Types Enhancement

Goal: deliver a minimal encrypted object store that leverages Aura's Keyhive capability system for access control, device key derivation for encryption, and session types for compile-time protocol safety. The storage system focuses on the core store-and-retrieve functionality with type-safe choreographic protocols, separated authentication/authorization, capability-checked manifests, encrypted chunk upload/download, and static replica lists.

This foundation establishes capability-based storage access, device key encryption, separated authentication/authorization, and session-typed choreographic protocols, providing a solid base for future enhancement with social features, proof-of-storage, and advanced eviction policies.

## Architectural Separation of Concerns

This Storage system integrates with three distinct but complementary Aura subsystems:

### **Journal/Ledger (`crates/journal/`)** - Account State Authority
**Role**: Intra-account consistency and protocol coordination
- ✅ **Already implemented** - handles account state with Automerge CRDTs
- ✅ **Threshold signatures** for critical operations (manifest signing)
- ✅ **Event sourcing** for account operations (device management, protocol sessions)
- ✅ **Session coordination** for distributed protocols
- ✅ **Capability authority graph** tracking and delegation chains
- **Storage MVP Integration**: Provides threshold signatures for manifests, capability verification, and protocol coordination

### **Store Crate (`crates/store/`)** - Local Storage Foundation
**Role**: Encrypted content storage and retrieval infrastructure
- ❌ **Currently skeleton** - target for MVP implementation
- **MVP Scope**: Chunking, content addressing, local indexing, quota management, capability-based access control
- **Explicitly Does NOT Handle**: Distributed replication, peer discovery, transport coordination
- **Storage Integration**: This specification extends the store crate with distributed capabilities

### **SBB System (051_rendezvous_ssb.md)** - Peer Discovery & Communication
**Role**: Inter-account rendezvous and social network coordination
- **Separate system** from storage - handles peer discovery and envelope flooding for communication
- **Storage Integration**: Provides peer discovery, trust bootstrapping, and relationship establishment for storage replica placement
- **Clear Boundary**: SBB handles "finding peers", Storage handles "storing content with peers"

### **Unified Architecture**

```
┌─────────────────────────────────────────────────────────────┐
│                        Agent Layer                          │
│              (Unified API for applications)                 │
└─────────────────────────┬───────────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────────┐
│              Unified Journal CRDT                           │
│         (Single Source of Truth for All State)             │
│                                                             │
│ • Core account state (devices, guardians, capabilities)    │
│ • Storage manifests and chunk metadata                     │
│ • SBB envelopes and neighbor management                    │
│ • Quota management and access control                      │
│ • Relationship keys and communication state                │
└─────────────────────────┬───────────────────────────────────┘
                          │
      ┌───────────────────┼───────────────────────────────────┐
      │        Shared Infrastructure:                         │
      │        • Keyhive Authority Graph (Authorization)      │
      │        • Device Key Derivation (Encryption)           │
      │        • Transport Layer (P2P Communication)          │
      │        • Session Types (Protocol Safety)              │
      └───────────────────┼───────────────────────────────────┘
                          │
   ┌────▼─────┐                                        ┌────▼─────┐
   │   Local  │                                        │ External │
   │ Storage  │                                        │ Network  │
   │(File Sys)│                                        │(P2P Mesh)│
   └──────────┘                                        └──────────┘
```

**Key Architectural Principle**: The Storage system is **fully integrated into the unified Journal CRDT** managed by the Keyhive authority graph, sharing the same single source of truth with all other subsystems including SBB for complete state consistency.

## 1. Core Concepts

- **Object Manifest** – threshold-signed, capability-controlled metadata with separated identity and permission key derivation.
- **Chunks** – encrypted data blocks using device-derived keys; chunking policy (1–4 MiB) determined by client type.
- **Type-Safe Storage Protocols** – session-typed choreographic protocols for storage operations ensuring compile-time protocol safety.
- **Separated Authentication/Authorization** – clean separation between device identity verification and permission management.
- **Unified Capability-Based Access** – leverages Keyhive's convergent capabilities with precise permission scoping.
- **Shared Peer Discovery** – unified peer discovery with use-case-specific selection (storage reliability vs communication reachability).
- **Integrated Transport** – shared transport infrastructure supporting both chunk operations and SBB envelope flooding.
- **Inline Metadata** – application metadata stored directly in the manifest for atomic writes.

**Session Types Integration Points**:
- Type-safe storage operation choreographies with compile-time protocol verification in `crates/coordination/src/storage_choreography.rs`
- Runtime witnesses for distributed storage conditions (replica thresholds, capability verification)
- Crash-safe protocol state rehydration from journal evidence using existing patterns in `crates/coordination/src/session_types/`

**SBB Integration Points**:
- Shared capability system for storage access and communication relay permissions
- Unified peer discovery and trust evaluation for both storage replicas and SBB neighbors
- Coordinated key derivation for storage encryption and communication relationship keys

**Future Enhancements (Post-MVP)**:
- Extended session-typed protocols for advanced storage choreographies
- Proof-of-Storage challenges and verification with session type safety
- Social replica placement using SBB trust graphs
- Trust-weighted quotas and social eviction policies
- Dynamic peer discovery and relationship-based storage
- Proxy re-encryption for capability delegation without key sharing

## 2. Data Structures

### 2.1 ObjectManifest

```rust
struct ObjectManifest {
    // Data
    root_cid: Cid,
    size: u64,
    chunking: ChunkingParams,
    erasure: Option<ErasureMeta>, // Optional, default None in MVP
                                  // Future: Tahoe-LAFS k-of-n erasure coding

    // Future: Chunk digests for proof-of-storage (Phase 2+)
    // chunk_digests: Option<Vec<[u8; 32]>>, // For proof-of-storage without full chunks

    // Context
    context_id: Option<[u8; 32]>,
    app_metadata: Option<Vec<u8>>, // e.g., CBOR, recommended max 4 KiB

    // Security & Access Control
    key_derivation: KeyDerivationSpec, // Separated identity and permission key derivation
    access_control: AccessControl,     // Precise capability-based access control

    // Lifecycle
    replication_hint: StaticReplicationHint, // Static peer list for MVP
    version: u64,
    prev_manifest: Option<Cid>,
    issued_at_ms: u64,
    created_at_epoch: u64, // Hook for snapshot-based GC
    nonce: [u8; 32],
    sig: ThresholdSignature,
}

// Authentication vs Authorization Separation
// Authentication: proves device identity via threshold signatures
struct DeviceAuthentication {
    device_id: DeviceId,
    account_id: AccountId,
    device_signature: ThresholdSignature,  // Proves "who you are"
}

// Authorization: defines what authenticated device can do
enum Permission {
    Storage { operation: StorageOp, resource: ResourceScope },
    Communication { operation: CommOp, relationship: RelationshipScope },
    Relay { operation: RelayOp, trust_level: TrustLevel },
}

// Clean capability token with precise separation
struct CapabilityToken {
    authenticated_device: DeviceId,        // Who (authentication)
    granted_permissions: Vec<Permission>,  // What (authorization)
    delegation_chain: Vec<CapabilityId>,   // Authority path
    signature: ThresholdSignature,         // Proof of grant
}

// MVP: Capability-based access control (no composite permissions)
enum AccessControl {
    CapabilityBased {
        required_permissions: Vec<Permission>,  // Precise permission requirements
        delegation_chain: Vec<CapabilityId>,    // Authority graph path
    },
}

// Future access control extensions (Phase 2+)
// ThresholdCapability { required_guardians, capability_scope, guardian_capabilities }
// DeviceList { devices } - for migration from legacy manifests

// Precise key derivation with separated identity and permission contexts
struct KeyDerivationSpec {
    identity_context: IdentityKeyContext,     // For identity-based keys
    permission_context: Option<PermissionKeyContext>,  // For permission-scoped keys
    derivation_path: Vec<u8>,    // Additional context bytes
    key_version: u32,            // For independent rotation per subsystem
    
    // Future: Proxy re-encryption support (Phase 2+)
    // proxy_reencryption_hint: Option<ProxyReencryptionHint>,
}

// Identity-based key derivation (authentication)
enum IdentityKeyContext {
    DeviceEncryption { device_id: DeviceId },
    RelationshipKeys { relationship_id: String },
    AccountRoot { account_id: AccountId },
}

// Permission-based key derivation (authorization)
enum PermissionKeyContext {
    StorageAccess { operation: String },
    CommunicationScope { scope: String },
    RelayPermission { relay_type: String },
}

enum CommKeyType {
    BoxKey,  // For envelope encryption (K_box)
    TagKey,  // For routing tag computation (K_tag)
    PskKey,  // For transport PSK (K_psk)
}

// MVP: Static replica placement with offline fallback
struct StaticReplicationHint {
    // Primary: Explicitly configured peer list (works offline)
    target_peers: Vec<PeerId>,

    // Required capability for storage providers
    required_capability: CapabilityScope,

    // Target number of replicas
    target_replicas: u32,

    // Fallback policy when SBB unavailable
    fallback_policy: ReplicaFallbackPolicy,
}

// Replica policy that works without SBB trust scores
enum ReplicaFallbackPolicy {
    // Use statically configured peer list
    StaticPeerList { peers: Vec<PeerId> },

    // Random selection from available peers
    RandomSelection { min_peers: u32 },

    // Local-only storage (no replication)
    LocalOnly,
}

// Future: Social replica placement (Phase 2+)
// Only activates when SBB trust metrics are available
// struct SocialReplicationHint {
//     trusted_neighbors: Vec<PeerId>,
//     relationship_weights: BTreeMap<PeerId, f64>,
//     min_trust_diversity: u32,
//
//     // Required: fallback to static policy when SBB unavailable
//     fallback_to_static: StaticReplicationHint,
// }

// Future: Optional trust-weighted logic (requires SBB)
struct TrustWeightedHint {
    // Only activate when SBB metrics available
    trust_requirements: Option<TrustRequirements>,
    social_preferences: Option<SocialPreferences>,
}
```

### 2.2 Unified Journal State Layout

All storage-related state is integrated into the main Journal CRDT managed by the Keyhive authority graph:

```rust
pub struct UnifiedAccountLedger {
    // --- Core Identity/Journal State ---
    pub devices: Map<DeviceId, DeviceInfo>,
    pub guardians: Map<GuardianId, GuardianInfo>,
    
    // --- Keyhive Capability State ---
    pub capabilities: Map<CapabilityId, Delegation>,
    pub revocations: Map<CapabilityId, Revocation>,

    // --- Storage State (now part of main ledger) ---
    pub storage_manifests: Map<Cid, ObjectManifest>,
    pub storage_quotas: Map<AccountId, Quota>,
    pub chunk_metadata: Map<ChunkId, ChunkInfo>,
    pub storage_refs: Map<Cid, ReferenceType>, // "pin:<device>" | "cache:<peer>"
    
    // Future: Proxy re-encryption state (Phase 2+)
    // pub proxy_reencryption_keys: Map<(SourceKeyId, TargetKeyId), ProxyKey>,
    // pub capability_transformations: Map<CapabilityId, ProxyTransformation>,

    // --- SBB State (now part of main ledger) ---
    pub sbb_envelopes: Map<Cid, SealedEnvelope>,
    pub sbb_neighbors: Set<PeerId>,
    pub relationship_keys: Map<RelationshipId, RelationshipKeys>,

    // --- Local Cache Indexes (materialized views) ---
    pub app_indexes: Map<AppId, Map<Hash, Set<Cid>>>, // Built from app_metadata
    pub gc_candidates: Map<Cid, GcCandidate>, // { reason, ready_at }
}
```

**Benefits of Unified State:**
- Single source of truth eliminates synchronization complexity
- Capability revocation immediately affects both storage access and SBB permissions
- Keyhive visibility index provides atomic access control across all subsystems
- No cross-CRDT consistency issues or race conditions

## 3. API Surface

```rust
pub struct PutOpts {
    pub class: StoreClass,                    // Owned or SharedFromFriend (no SocialStorage initially)
    pub pin: PinClass,                        // Pin or Cache (no SocialReplica initially)
    pub repl_hint: StaticReplicationHint,     // Static peer list with offline fallback
    pub context: Option<ContextDescriptor>,
    pub app_metadata: Option<Vec<u8>>,        // Inline metadata blob
    pub access_control: AccessControl,        // Keyhive capability-based access
    pub key_derivation: KeyDerivationSpec,    // Device key derivation parameters

    // Future: Social and advanced features (Phase 2+)
    // pub trust_weighted_hint: Option<TrustWeightedHint>, // Only when SBB available
    // pub erasure_params: Option<ErasureParams>,
    // pub threshold_policy: Option<ThresholdPolicy>,
}

pub async fn store_encrypted(
    &self,
    payload: &[u8],
    device_auth: DeviceAuthentication,     // Who is making the request
    required_permissions: Vec<Permission>, // What permissions are needed
    opts: PutOpts,
) -> Result<Cid>;

pub async fn fetch_encrypted(
    &self,
    cid: &Cid,
    device_auth: DeviceAuthentication,     // Who is requesting
    opts: GetOpts,
) -> Result<(Vec<u8>, ObjectManifest)>;

pub async fn grant_storage_capability(
    &self,
    cid: &Cid,
    grantee_device: DeviceId,              // Who to grant to
    granted_permissions: Vec<Permission>,   // What to grant
) -> Result<CapabilityToken>;
```

Everything else (pin/unpin, eviction, quota reports) uses session-typed capability-checked access control implemented in `crates/coordination/src/storage_session.rs`.

## 4. Session-Typed Storage Protocols

Storage operations leverage session types for compile-time protocol safety with choreographic coordination:

**Core Session-Typed Protocols:**

```rust
// Session-typed storage protocol states
StorageProtocol<Initializing> → StorageProtocol<ManifestSigning> → 
StorageProtocol<ChunkEncryption> → StorageProtocol<ReplicaCoordination> → 
StorageProtocol<PermissionVerification> → StorageProtocol<Completed>

// Integration: crates/coordination/src/storage_choreography.rs
impl StorageProtocol<ManifestSigning> {
    pub fn sign_with_threshold(
        self, 
        witness: ThresholdSignaturesMet  // Runtime witness
    ) -> StorageProtocol<ChunkEncryption>
}

// Replica coordination protocol
ReplicaProtocol<PeerSelection> → ReplicaProtocol<CapabilityVerification> → 
ReplicaProtocol<ChunkDistribution> → ReplicaProtocol<ConfirmationCollection> → 
ReplicaProtocol<Completed>

// Integration: crates/coordination/src/replica_choreography.rs
impl ReplicaProtocol<CapabilityVerification> {
    pub fn verify_storage_permissions(
        self, 
        witness: StoragePermissionsVerified  // Runtime witness
    ) -> ReplicaProtocol<ChunkDistribution>
}
```

**Session Type Runtime Witnesses:**
- `ThresholdSignaturesMet`: Proves M-of-N signatures collected for manifest
- `StoragePermissionsVerified`: Proves peers have required storage permissions
- `ReplicationThresholdMet`: Proves sufficient replicas stored
- `CapabilityTokenValidated`: Proves capability token is valid and authorized

**Protocol State Rehydration:** Storage protocols can recover from crashes by analyzing journal evidence in `crates/journal/src/ledger.rs` to reconstruct valid session states.

## 5. Unified Transport Architecture (SBB Integration)

**Integration**: Shared transport infrastructure supporting both storage chunk operations and SBB envelope flooding with unified capability-based authorization.

**Unified Components**:
1. **UnifiedTransport** – Single transport abstraction for storage chunks and SBB envelopes
2. **Shared Peer Discovery** – Common peer discovery with use-case-specific selection criteria
3. **Unified Device Authentication** – Single device certificate system for all operations
4. **Integrated Capability Verification** – Shared capability checking for storage access and communication relay
5. **Connection Pool Management** – Shared connection handling across storage and SBB operations

**Transport Layer Responsibilities (Authentication Only)**:
1. **Device Authentication** – Verify device signatures and establish authenticated channels
2. `push_chunk` – Send encrypted chunk to authenticated peers (no authorization decisions) using session-typed protocols
3. `fetch_chunk` – Retrieve chunk from authenticated peers (no authorization decisions) with session type safety
4. **Basic Health Reporting** – Peer availability and connection status

**Application Layer Responsibilities (Authorization Only)**:
1. **Permission Verification** – Check if authenticated device has required permissions using session-typed capability protocols
2. **Capability Management** – Grant, revoke, and delegate permissions with type-safe state transitions
3. **Access Control Enforcement** – Allow/deny operations based on permissions with compile-time safety
4. **Policy Evaluation** – Apply business logic for authorization decisions through session-typed choreographies

**Session-Typed Transport Interface (Authentication Only):**

```rust
// Transport layer: only handles device authentication with session type safety
impl SessionTypedAuthenticatedTransport {
    // Storage operations - transport only authenticates using session types
    pub async fn push_chunk<S: StorageState>(
        &self,
        chunk: EncryptedChunk,
        target: PeerId,
        device_signature: DeviceSignature,  // Authentication proof
        protocol_state: StorageProtocol<S>,
    ) -> Result<StorageProtocol<S::NextState>>;

    pub async fn fetch_chunk(
        &self,
        chunk_id: ChunkId,
        from_peer: PeerId,
        device_signature: DeviceSignature,  // Authentication proof
    ) -> Result<EncryptedChunk>;

    // SBB operations - transport only handles identity verification
    pub async fn publish_envelope(
        &self,
        envelope: SbbEnvelope,
        device_signature: DeviceSignature,  // Who is publishing
    ) -> Result<()>;

    pub async fn subscribe_envelopes(
        &self,
        recognition_filter: EnvelopeFilter,
        device_signature: DeviceSignature,  // Who is subscribing
    ) -> Result<EnvelopeStream>;

    // Peer discovery - no authorization decisions at transport level
    pub async fn discover_peers(
        &self,
        peer_requirements: PeerRequirements,
    ) -> Result<Vec<PeerId>>;

    // Establish authenticated channel only
    pub async fn establish_authenticated_channel(
        &self,
        peer: PeerId,
    ) -> Result<AuthenticatedChannel>;
}

// Application layer: handles authorization decisions with session types
impl SessionTypedPermissionEnforcement {
    // Check if authenticated device has required permissions using session types
    pub async fn verify_permissions<S: CapabilityState>(
        &self,
        channel: &AuthenticatedChannel,
        required_permissions: Vec<Permission>,
        capability_protocol: CapabilityProtocol<S>,
    ) -> Result<(bool, CapabilityProtocol<S::NextState>)>;
    
    // Grant permissions to authenticated device with session type safety
    pub async fn grant_permissions<S: CapabilityGrantState>(
        &self,
        target_device: DeviceId,
        permissions: Vec<Permission>,
        grant_protocol: CapabilityGrantProtocol<S>,
    ) -> Result<(CapabilityToken, CapabilityGrantProtocol<S::NextState>)>;
}
```

## 5. Basic Quota & Eviction

**Capability-Based Quotas**:
- **Simple Counters**: Track storage quotas per capability scope
- **Basic Limits**: Per-peer storage limits from configuration
- **Capability Verification**: Only capability holders can store/access content

**Basic Eviction Strategy**:
- **Simple LRU**: Evict oldest cached content when limits exceeded
- **Capability-Aware**: Preserve data for active capability holders
- **Manual Cleanup**: Basic garbage collection of expired content

**Basic Management**:
```rust
pub struct BasicQuotaManager {
    // Storage quotas per permission type (authorization)
    permission_quotas: BTreeMap<Permission, u64>,

    // Per-device storage limits (authentication-based)
    device_limits: BTreeMap<DeviceId, u64>,

    // Simple LRU eviction
    eviction_policy: LruEvictionPolicy,
}
```

**Future Enhancements (Phase 2+)**:
- Trust-weighted quotas and social eviction policies
- Proof-of-storage challenges and verification
- Coordinated cleanup across replica sets

## 6. Basic Deletion & Revocation

**Capability-Based Deletion**:
1. **Capability Revocation** – Revoke storage capabilities, making content inaccessible
2. **Device Key Rotation** – Rotate derived keys using device key manager
3. **Local Cleanup** – Remove local chunks and update capability grants
4. **Cryptographic Erasure** – Combined capability revocation + key rotation

**Basic Deletion Modes**:
1. **Local Eviction** – Remove local chunks while preserving audit trail
2. **Capability Revocation** – Remove access permissions via Keyhive
3. **Cryptographic Erasure** – Revoke capabilities and rotate keys
4. **Manual Cleanup** – Best-effort deletion with basic confirmation

**Future Enhancements (Phase 2+)**:
- Trust-weighted quotas and social eviction policies (requires SBB trust metrics)
- Coordinated deletion across social network
- Proof-of-storage with manifest-stored digests (see corrected design below)
- Automatic replica cleanup choreography
- Proxy re-encryption for efficient key rotation without full re-encryption using [rust-umbral](https://github.com/nucypher/rust-umbral)

## 6.1. Corrected Proof-of-Storage Design (Future)

**Problem with Original Design**: The original proof-of-storage challenge required verifiers to have the full chunk locally to recompute `hash(chunk || replica_tag || ...)`, defeating the purpose of offloading storage.

**Corrected Design**: Store chunk digests in the manifest and base challenges on those digests:

```rust
// Future: Corrected proof-of-storage (Phase 2+)
struct ProofOfStorageChallenge {
    // Stored in manifest (not requiring full chunk)
    chunk_digest: [u8; 32],        // Fixed hash of the chunk content

    // Challenge components
    freshness_nonce: [u8; 16],     // Prevents replay attacks
    capability_id: CapabilityId,   // Binds to authorization context
    challenge_timestamp: u64,      // For freshness verification
}

struct ProofOfStorageResponse {
    // Replica can respond without coordinator having full chunk
    challenge_signature: Vec<u8>,  // Sign(chunk_digest || nonce || capability_id)
    storage_proof: Vec<u8>,        // Cryptographic proof of storage
}

// Verification process:
// 1. Coordinator sends challenge with manifest-stored chunk_digest
// 2. Replica signs the challenge using stored chunk + nonce
// 3. Coordinator verifies signature against known chunk_digest
// 4. No need for coordinator to rehydrate full chunk payload
```

**Benefits**:
- Coordinator doesn't need full chunk for verification
- Freshness nonce prevents replay attacks
- Capability binding ensures authorized verification
- Scales to large chunks without bandwidth overhead

## 7. Inline Metadata Guidance

- Keep metadata blobs under 4 KiB to avoid inflating manifests.
- For high-frequency updates (chat messages), batch metadata (e.g., journaling) where possible.
- Indexer hashes key/value pairs internally (`BLAKE3`) for query indexes; plaintext comparisons happen client-side.

## 8. Phase 1 Scope & Implementation Plan

**In Scope (Initial Implementation)**:
- Capability-based access control using implemented Keyhive system
- Device key derivation for storage encryption using existing DeviceKeyManager
- Basic transport using existing CapabilityTransport infrastructure
- Static replica lists for chunk placement
- Simple quota management and LRU eviction

**Explicitly Out of Scope (Future Phases)**:
- Proof-of-storage challenges and verification (requires mature telemetry)
- Social replica placement using SBB trust graphs (requires SBB maturity)
- Trust-weighted quotas and social eviction policies (requires trust metrics)
- Distributed revocation choreography (requires coordination protocols)
- Erasure coding policies (leave `erasure` set to `None`)
- Threshold guardian approval for high-value content

**Implementation Strategy: Extend Existing Store Crate**

This specification should be implemented as an **extension of the existing `crates/store/` crate**, not as a replacement. The current store crate provides a solid foundation with:
- ✅ **CapabilityStorage** with authority graph integration
- ✅ **Proof-of-storage challenge system** (ready for future use)
- ✅ **Basic quota tracking** and LRU eviction
- ✅ **BLAKE3 content integrity** verification

**Implementation Plan (Revised: 5 weeks total)**:

### Sprint 1: Manifest Signing + Basic Upload (2 weeks)
**Week 1: Foundation**
- **Extend** existing CapabilityStorage with ObjectManifest structure
- **Integrate** existing device key derivation for chunk encryption keys
- **Add** manifest signing with threshold signatures (leveraging journal integration)
- **Exit Criteria**: Can create and sign valid manifests using existing capability foundation
- **Rollback Point**: Revert to existing CapabilityStorage if manifest validation fails

**Week 2: Basic Storage**
- **Enhance** existing chunk storage with encryption using causal encryption
- **Extend** existing store operations with static peer configuration
- **Build upon** existing capability verification for storage access
- **Exit Criteria**: Can store and retrieve single chunks locally using extended store crate
- **Rollback Point**: Disable new chunk operations if encryption extension fails

### Sprint 2: Single-Peer Replication (1.5 weeks)
- Basic chunk upload to configured static peers
- Simple peer-to-peer chunk transfer using existing transport
- Offline fallback policies when peers unavailable
- **Exit Criteria**: Can replicate chunks to one configured peer
- **Rollback Point**: Fall back to local-only storage if replication fails

### Sprint 3: Capability Enforcement (1 week)
- Integrate with existing Keyhive capability system
- Capability-based access control for fetch operations
- Basic quota tracking per capability scope
- **Exit Criteria**: Can enforce capability-based access to stored content
- **Rollback Point**: Temporarily disable access control if capability integration breaks

### Sprint 4: System Hardening (0.5 weeks)
- Comprehensive testing of capability-based storage access
- Error handling and edge case management
- Basic performance optimization
- **Exit Criteria**: Stable system ready for limited deployment

**Prerequisites for Future Phases**:
- **Phase 2 (SBB Enhancement)**: Requires mature SBB trust metrics, relationship tracking, and extended session types
- **Phase 3 (Proof-of-Storage)**: Requires session-typed proof-of-storage choreography with manifest-stored digests
- **Phase 4 (Advanced Features)**: Requires distributed coordination protocols, economic incentives, and advanced session type patterns

**Session Type Integration Points:**
- **Core Infrastructure**: `crates/coordination/src/session_types/mod.rs` - Base session type traits
- **Storage Protocols**: `crates/coordination/src/storage_choreography.rs` - Storage-specific session types
- **Replica Coordination**: `crates/coordination/src/replica_choreography.rs` - Replica management session types
- **Agent Integration**: `crates/agent/src/agent.rs` - Session-typed storage operations
- **Transport Integration**: `crates/transport/src/capability_transport.rs` - Session-typed transport operations

## 8.1 SBB Integration Benefits

**Unified Architecture Advantages**:
1. **Single Capability System**: One authority graph manages storage access, communication relay permissions, and peer trust evaluation
2. **Shared Peer Discovery**: Communication relationships bootstrap storage trust, reducing cold-start problems
3. **Coordinated Key Management**: Unified key derivation with clear context separation prevents key confusion
4. **Integrated Transport**: Single connection pool serves both chunk transfers and envelope flooding
5. **Consistent Trust Model**: Social relationships drive both storage replica placement and communication routing

**Reduced Complexity**:
- Single transport authentication mechanism
- Shared neighbor management and connection handling
- Unified CRDT infrastructure (separate documents, shared replication)
- Consistent error handling and retry logic across subsystems

**Enhanced Security**:
- Capability-based access control applied uniformly
- Independent key rotation with coordinated capability revocation
- Social accountability across both storage and communication
- Unified audit trail for all peer interactions

## 9. Architectural Decision: Unified State Model

### 9.1 Planning for Garbage Collection

To ensure that the storage system can be efficiently garbage collected in the future, even without a full GC system in Phase 1, the following design principles are included:

1.  **Epoch-Tagged Manifests**: Every `ObjectManifest` includes a `created_at_epoch` field. This ties the manifest to the logical clock of the unified `Journal` CRDT. A future snapshot-based GC system can use this epoch to definitively prove that a manifest (and its associated data) existed before a snapshot, making it a candidate for pruning if it has been logically deleted.

2.  **Centralized Reference Tracking**: The `UnifiedAccountLedger` is the single source of truth for which data is active. The `storage_manifests` map contains all active manifests. A future "mark and sweep" GC process can traverse this map to determine the complete set of referenced chunks. Any chunk not in this set is an orphan and can be collected.

By embedding the epoch and relying on the centralized journal for reference tracking from the start, we provide the necessary hooks to build a safe and efficient garbage collector later without requiring significant architectural changes.

### **Response to Feedback: True State Unification**

The original design shared infrastructure (capabilities, transport, session types) but maintained separate CRDT documents:
- Journal CRDT for account state
- Storage indexes for content metadata  
- SBB Document for communication state

**Identified Problem**: This separation created synchronization complexity, potential inconsistencies during partitions, and cognitive overhead from reasoning about multiple interacting state machines.

### **Unified Solution**

All storage state is now integrated into the **main Journal CRDT** managed by the Keyhive authority graph:

```rust
pub struct UnifiedAccountLedger {
    // --- Core Identity/Journal State ---
    pub devices: Map<DeviceId, DeviceInfo>,
    pub guardians: Map<GuardianId, GuardianInfo>,
    
    // --- Keyhive Capability State ---
    pub capabilities: Map<CapabilityId, Delegation>,
    pub revocations: Map<CapabilityId, Revocation>,

    // --- Storage State (now part of main ledger) ---
    pub storage_manifests: Map<Cid, ObjectManifest>,
    pub storage_quotas: Map<AccountId, Quota>,
    pub chunk_metadata: Map<ChunkId, ChunkInfo>,

    // --- SBB State (also unified) ---
    pub sbb_envelopes: Map<Cid, SealedEnvelope>,
    pub sbb_neighbors: Set<PeerId>,
    pub relationship_keys: Map<RelationshipId, RelationshipKeys>,
}
```

### **Atomic Consistency Benefits**

1. **Single Source of Truth**: Storage capability grants and SBB relay permissions live in the same authority graph
2. **Immediate Revocation**: When capabilities are revoked, storage access and relay permissions are atomically consistent  
3. **Unified Access Control**: Keyhive visibility index controls materialization across all subsystems
4. **Simplified Implementation**: One state model instead of complex cross-CRDT synchronization

### **Preserved Design Benefits**

- Clean separation of concerns at the API level
- Independent key rotation with coordinated capability management
- Session-typed protocol safety across all operations
- Choreographic programming model for distributed coordination

This architectural change moves beyond infrastructure sharing to true state unification as recommended in the feedback.

## 10. Future Enhancement Roadmap

The unified architecture naturally evolves toward sophisticated distributed storage capabilities:

### Phase 2: Social Storage Network (4 weeks)
- **Web-of-Trust Storage**: Extend SBB nodes to provide storage capacity to trusted contacts
- **Trust-Based Replication**: Use social graph for intelligent replica placement across relationship boundaries
- **Capability-Driven Social Quotas**: Manage storage permissions through convergent capabilities with social backing
- **Relationship-Aware Caching**: Optimize cache placement using SBB relationship strength and interaction patterns
- **Proxy Re-encryption Integration**: Enable capability delegation and key rotation without exposing plaintext to storage providers using [rust-umbral](https://github.com/nucypher/rust-umbral) threshold proxy re-encryption

### Phase 3: Encrypt-then-Erasure-Code (6 weeks)
- **Privacy-First Erasure Coding**: Implement Tahoe's encrypt-before-encode pattern with capability-derived keys
- **Meaningless Fragments**: Storage nodes see only encrypted erasure-coded shares, maintaining privacy
- **Reed-Solomon Integration**: Add k-of-n reconstruction with configurable parameters and social replica distribution
- **Capability-Based Reconstruction**: Require specific capabilities for erasure code reconstruction operations

### Phase 4: Threshold + Social Storage Synergy (8 weeks)
- **Guardian-Encrypted High-Value Storage**: Combine threshold cryptography with social storage for sensitive content
- **Hybrid Security Model**: Layer guardian approval, capability-based access, and social trust for comprehensive protection
- **Cross-Network Recovery**: Use both guardian recovery and social storage network for resilience
- **Unified Authority**: Single Keyhive authority graph manages guardian permissions, storage access, and social trust

### Phase 5: Advanced Privacy & Performance (Ongoing)
- **Anonymous Social Storage**: Privacy-preserving storage using capability tokens without identity revelation
- **Hierarchical Trust Networks**: Multi-level trust graphs for scaling beyond immediate social connections
- **Economic Incentives**: Integration with economic mechanisms for storage provision and consumption
- **Formal Verification**: Mathematical verification of security properties across the unified system

### Architectural Evolution Principles:
1. **Unified Foundation**: All enhancements build on the Keyhive + SBB + Device Key integration
2. **Social-First Design**: Social relationships drive storage decisions, with cryptography providing security guarantees
3. **Incremental Privacy**: Each phase adds privacy protections without breaking existing functionality
4. **Capability Consistency**: Single authority graph manages all permissions across storage, messaging, and social coordination
5. **Trust Network Effects**: Storage reliability improves as social network grows and relationships strengthen

This roadmap leverages the unified architecture implemented in Phase 1 to create a uniquely integrated local-first storage system that combines social trust, cryptographic security, and distributed resilience.
