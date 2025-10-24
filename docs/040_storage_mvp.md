# 040 · Storage MVP Specification (Phase 1)

Goal: deliver a minimal encrypted object store that pairs with the identity
layer. We rely on an existing transport (iroh or HTTPS relay) and add just enough orchestration to meet Aura's UX/security needs.

This MVP establishes the foundation for future Tahoe-LAFS inspired enhancements including capability-based access control, encrypt-then-erasure-code patterns, and social storage networks built on the web-of-trust.

## 1. Core Concepts

- **Object Manifest** – signed, inline metadata, single logical write per user action.
- **Chunks** – encrypted data blocks; chunking policy (1–4 MiB) determined by client type.
- **Proof-of-Storage** – challenge/response to confirm peers actually store assigned chunks.
- **Quota & Eviction** – simple byte counters + LRU per peer; no multi-tier caches in MVP.
- **Inline Metadata** – application metadata stored directly in the manifest for atomic writes.

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

    // Context
    context_id: Option<[u8; 32]>,
    app_metadata: Option<Vec<u8>>, // e.g., CBOR, recommended max 4 KiB

    // Security & Access Control
    key_envelope: KeyEnvelope,     // HPKE-wrapped per device (MVP)
                                   // Future: Capability-based key distribution
    access_control: AccessControl, // Replaces auth_token_ref for Keyhive integration

    // Lifecycle
    replication_hint: ReplicationHint, // Future: Social storage network hints
    version: u64,
    prev_manifest: Option<Cid>,
    issued_at_ms: u64,
    nonce: [u8; 32],
    sig: ThresholdSignature,
}

// MVP: Simple enum, extensible for future capability-based access
enum AccessControl {
    // MVP: Device-based access (current approach)
    DeviceList { devices: Vec<DeviceId> },
    
    // Future: Capability-based access (Tahoe-LAFS inspired)
    CapabilityRef { 
        capability_id: CapabilityId,
        access_type: AccessType, // Read, Write, Verify
    },
    
    // Future: Threshold access (high-value content)
    ThresholdAccess {
        required_guardians: u32,
        total_guardians: u32,
        guardian_shares: Vec<GuardianShare>,
    },
}
```

### 2.2 Local Index Layout

```
/manifests/<cid>          -> Serialized ObjectManifest
/chunks/<cid>/<chunk_id>  -> { state, size, last_access }
/refs/<cid>               -> "pin:<device>" | "cache:<peer>"
/space/account/<account>  -> { pinned_bytes, cached_bytes, limits }
/space/peer/<peer>        -> { cached_bytes, last_updated }
/index/app/<app_id>/<hash>-> Set<Cid> (built from app_metadata)
/gc/candidates            -> { cid, reason, ready_at }

// Future: Capability-based access indexes
/capabilities/<cap_id>    -> { access_type, delegations, revocations }
/erasure/<cid>           -> { k, n, share_locations } (Tahoe-LAFS style)
/social_storage/<peer>   -> { trust_level, storage_quota, reliability }
```

## 3. API Surface (Indexer)

```rust
pub struct PutOpts {
    pub class: StoreClass,            // Owned or SharedFromFriend
    pub pin: PinClass,                // Pin or Cache
    pub repl_hint: ReplicationHint,   // Preferred peers (optional target list)
    pub context: Option<ContextDescriptor>,
    pub app_metadata: Option<Vec<u8>>, // Inline metadata blob
    pub access_control: AccessControl, // Replaces caps for Keyhive integration
    
    // Future: Tahoe-LAFS inspired options
    pub erasure_params: Option<ErasureParams>, // k-of-n coding parameters
    pub threshold_policy: Option<ThresholdPolicy>, // Guardian approval requirements
}

pub async fn store_encrypted(
    &self,
    payload: &[u8],
    recipients: Recipients,
    opts: PutOpts,
) -> Result<Cid>;

pub async fn fetch_encrypted(
    &self,
    cid: &Cid,
    opts: GetOpts,
) -> Result<(Vec<u8>, ObjectManifest)>;
```

Everything else (pin/unpin, eviction, quota reports) mirrors existing APIs.

## 4. Transport Adapter (Phase 1)

- **Default**: iroh adapter (QUIC/WebTransport) or HTTPS relay in constrained environments.
- **Responsibilities**:
  1. Secure channel (Noise/TLS/DTLS) enforcing presence tickets.
  2. `push_chunk` – send encrypted chunk to candidate peers.
  3. `fetch_chunk` – retrieve chunk on demand.
  4. `verify_chunk_presence` – send challenge nonce, return `ReplicaProof { replica_tag, signature }`.
  5. Health reporting (peer count, last sync).

Pluggable structure is retained, but additional transports are future work.

## 5. Proof-of-Storage Flow

1. Indexer selects target peers per `ReplicationHint`.
2. Transport pushes chunk; each replica records a `ReplicaTag` (UUID).
3. Indexer issues challenge (`chunk_cid`, `nonce`).
4. Replica computes `hash(chunk || replica_tag || nonce)`, signs with device key.
5. Transport returns `ReplicaProof`.
6. Indexer verifies signature (including the claimant’s current `session_epoch` and `ticket_digest = BLAKE3(presence_ticket_bytes)`), stores `replica_tag` for future tombstone, and rejects proofs that present a stale epoch.

### 5.1 Replica Revocation & Epoch Bumps

- Replica responses MUST include the issuer’s `session_epoch` in the signed payload (`hash(chunk || replica_tag || nonce || session_epoch)`), binding proofs to the epoch that authorised the replica.
- On `SessionEpochBump`, the indexer invalidates cached presence-ticket digests and challenges replicas; any proof that still refers to the old epoch is treated as revoked.
- When a device is removed (CRDT tombstone), its associated `replica_tag`s are enqueued in `/gc/candidates` so background jobs stop scheduling challenges and trigger chunk re-replication if needed.

## 6. Quota & Eviction

- **Counters**: track pinned and cached bytes per account + peer.
- **Eviction**: single-tier LRU per peer (evict oldest cached object).
- **Triggers**: background job scans `/gc/candidates` and evicts as needed.
- **Configuration**: per-account total limit, per-peer cache limit (config file).

## 7. Deletion Modes

1. **Local Eviction** – delete local chunks, keep tombstone.
2. **Cryptographic Erasure** – rotate `key_envelope`, publish revocation manifest.
3. **Secure Wipe** – best-effort physical delete (native) or rely on erasure (browser).

## 8. Inline Metadata Guidance

- Keep metadata blobs under 4 KiB to avoid inflating manifests.
- For high-frequency updates (chat messages), batch metadata (e.g., journaling) where possible.
- Indexer hashes key/value pairs internally (`BLAKE3`) for query indexes; plaintext comparisons happen client-side.

## 9. Out-of-Scope (Phase 1)

- Friend caching tiers, gossip-based cache distribution.
- Erasure coding policies (leave `erasure` set to `None`).
- Alternate transports (BitChat mesh, libp2p). Documented as future modules.
- Automated garbage-collection of remote replicas (rely on proof-of-storage and manual policies).

## 10. Future Enhancement Roadmap (Tahoe-LAFS Integration)

The MVP architecture is designed to seamlessly evolve toward Tahoe-LAFS inspired capabilities:

### Phase 2: Capability-Based Access Control
- **Keyhive Integration**: Replace device-list access with convergent capabilities
- **Delegation Chains**: Support read/write/verify capability types inspired by Tahoe-LAFS
- **Revocation Cascades**: Leverage Keyhive's authority graph for complex revocation scenarios

### Phase 3: Encrypt-then-Erasure-Code
- **Privacy-First Erasure Coding**: Implement Tahoe's encrypt-before-encode pattern
- **Meaningless Fragments**: Storage nodes see only encrypted erasure-coded shares
- **Reed-Solomon Integration**: Add k-of-n reconstruction with configurable parameters

### Phase 4: Social Storage Network
- **Web-of-Trust Storage**: Extend SBB nodes to provide storage capacity
- **Trust-Based Replication**: Use social graph for intelligent replica placement
- **Capability-Driven Quotas**: Manage storage permissions through convergent capabilities

### Phase 5: Threshold + Erasure Synergy
- **Guardian-Encrypted Shares**: High-value content requires M-of-N guardian approval
- **Hybrid Security Model**: Combine threshold cryptography with erasure coding
- **Unified Authority**: Single Keyhive authority graph manages both access and encryption

### Key Design Principles for Future Phases:
1. **Backward Compatibility**: All enhancements preserve existing manifest structures
2. **Incremental Adoption**: Features can be enabled per-object via `AccessControl` enum
3. **Unified Architecture**: Keyhive provides consistent foundation for both authorization and encryption
4. **Privacy by Design**: Following Tahoe-LAFS principle that privacy and fault tolerance reinforce each other

Once the MVP lands, we can iterate on these enhancement modules without changing the base APIs captured here.
