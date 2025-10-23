# 040 · Storage MVP Specification (Phase 1)

Goal: deliver a minimal encrypted object store that pairs with the identity
layer. We rely on an existing transport (iroh or HTTPS relay) and add just enough orchestration to meet Aura’s UX/security needs.

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

    // Context
    context_id: Option<[u8; 32]>,
    app_metadata: Option<Vec<u8>>, // e.g., CBOR, recommended max 4 KiB

    // Security
    key_envelope: KeyEnvelope,     // HPKE-wrapped per device
    auth_token_ref: Option<Cid>,   // Biscuit capability for delegated access

    // Lifecycle
    replication_hint: ReplicationHint,
    version: u64,
    prev_manifest: Option<Cid>,
    issued_at_ms: u64,
    nonce: [u8; 32],
    sig: ThresholdSignature,
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
```

## 3. API Surface (Indexer)

```rust
pub struct PutOpts {
    pub class: StoreClass,            // Owned or SharedFromFriend
    pub pin: PinClass,                // Pin or Cache
    pub repl_hint: ReplicationHint,   // Preferred peers (optional target list)
    pub context: Option<ContextDescriptor>,
    pub app_metadata: Option<Vec<u8>>, // Inline metadata blob
    pub caps: Vec<WillowCap>,         // Biscuit tokens if needed
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

Once the MVP lands, we can iterate on optional modules without changing the base APIs captured here.***
