# Rendezvous & Social Bulletin Board (SBB)

**Transport-agnostic private reconnection over a web of trust with session type safety**

> **Note:** This document has been updated to align with the existing Aura architecture (080_...) and enhanced with session types for compile-time protocol safety. The SBB protocol fills a major gap in the current Aura design: how one Aura user finds and establishes a secure channel with another Aura user. The core identity concepts are so aligned that integration is not only possible but highly desirable.

## Architectural Separation of Concerns

The SBB system is a **peer discovery and communication layer** that integrates with but remains distinct from Aura's other storage systems:

### **SBB's Core Responsibility: Inter-Account Communication**
**Role**: Private peer discovery and relationship establishment
- **Envelope flooding** for rendezvous coordination
- **Relationship key management** for inter-account communication
- **Peer discovery and trust bootstrapping** for storage and communication
- **Social web-of-trust coordination** across account boundaries
- **Clear Boundary**: SBB handles "finding and connecting to peers", not "storing content"

### **Unified State Architecture**

```
┌────────────────────────────────────────────────────────────────┐
│                      Application Layer                         │
│         (Unified API: "store content with trusted peers")      │
└──────────────────────────┬─────────────────────────────────────┘
                           │
┌──────────────────────────▼─────────────────────────────────────┐
│                 Unified Journal CRDT                           │
│            (Single Source of Truth for All State)             │
│                                                                │
│ • Account state (devices, guardians, capabilities)            │
│ • SBB envelopes and neighbor management                       │
│ • Storage manifests and chunk metadata                       │
│ • Relationship keys and communication state                   │
│ • Quota management and access control                        │
│ • Session epochs and presence management                     │
└──────────────────────────┬─────────────────────────────────────┘
                           │
        ┌──────────────────┼──────────────────────────────────────┐
        │         Shared Infrastructure:                           │
        │         • Keyhive Authority Graph (Authorization)        │
        │         • Transport Layer (P2P Communication)            │
        │         • Session Types (Protocol Safety)               │
        │         • Device Key Derivation (Encryption)             │
        └──────────────────┼──────────────────────────────────────┘
                           │
      ┌────▼─────┐                                        ┌─────▼────┐
      │ Network  │                                        │  Local   │
      │ (P2P     │                                        │ Storage  │
      │ Gossip)  │                                        │ (Files)  │
      └──────────┘                                        └──────────┘
```

### **Data Flow: SBB → Storage Integration**

1. **SBB discovers peers** through envelope flooding and social relationships
2. **SBB establishes trust** via relationship key exchange and capability verification
3. **SBB provides peer list** to Storage for replica placement decisions
4. **Storage stores content** using SBB-discovered peers with separate storage-specific protocols
5. **Both systems share** the underlying capability system, transport layer, and session types

### **Unified State Model**

| Data Type | Location in Unified Journal CRDT | Purpose |
|-----------|----------------------------------|---------|
| **Account State** | `devices`, `guardians`, `capabilities` | Core identity and authorization |
| **SBB Envelopes** | `sbb_envelopes: Map<Cid, SealedEnvelope>` | Rendezvous coordination |
| **SBB Neighbors** | `sbb_neighbors: Set<PeerId>` | Active neighbor management |
| **Relationship Keys** | `relationship_keys: Map<RelationshipId, Keys>` | Inter-account communication |
| **Storage Manifests** | `storage_manifests: Map<Cid, ObjectManifest>` | Content metadata |
| **Storage Quotas** | `storage_quotas: Map<AccountId, Quota>` | Resource management |

**Critical Design Principle**: SBB state is **fully integrated into the main Journal CRDT** managed by the Keyhive authority graph, eliminating separate state management and ensuring single source of truth for all account and communication state.

---

## 1) Motivation

Aura accounts are threshold identities (account-level) with many devices underneath. After two accounts connect once, they should be able to:

* go offline, change networks/devices/transports,
* and still find each other privately and re-establish a channel,
* *without* revealing their social graph or stable identifiers to any relays.

We achieve this by flooding small, sealed, fixed-size envelopes through a social bulletin board (SBB) that’s hosted by your contacts (and optionally their contacts) rather than by a central relay. Only the intended counterparty can recognize and decrypt an envelope; everyone else stores/forwards it blindly with quotas.

---

## 2) Goals & Non-Goals

**Goals**

* Transport-agnostic rendezvous (QUIC, WebRTC, Tor, BLE…).
* Privacy: no long-lived global IDs in discovery (approximately 1 day); pairwise unlinkability.
* Offline/partition tolerance via store-and-forward.
* Core Integration: Deeply integrated with Aura's threshold identity model and multi-device architecture.
* Storage Integration: Unified with Aura's storage system for capability management and peer discovery.
* Session Type Safety: Compile-time verification of envelope protocols and rendezvous choreographies.
* Minimal infra: run by users/contacts (WoT-flood), not centralized servers.
* Practical: path to a working multi-device PoC demonstrating Aura's unique value proposition.

**Non-Goals (MVP)**

* Global anonymity against nation-state traffic correlation.
* Multi-hop onion routing of the data plane.
* Complex cryptographic DoS protection (web of trust provides natural Sybil resistance).
* Advanced membership protocols (start with static neighbor lists).
* Sophisticated key rotation and recovery mechanisms.

---

## 3) System Overview (Components)

**A. Aura Account & Device Identity**

* **Aura Account** = threshold public key (Group Public Key) originating from multiple devices (e.g., Ed25519-FROST from phone and laptop) + policy.
* **Device** = per-device static keypair (Noise/HPKE) with a short **device certificate** signed by the account's threshold signature scheme (TSS).

**B. Pairwise Relationship**

* When two Aura accounts connect, they mint a **pairwise secret** `RID_AB` via per-device Diffie-Hellman key exchange, not the aggregated Group Public Keys.
* **Key Agreement Process**: Each side picks a designated "link device" and performs X25519 DH using the device's existing Noise/HPKE static key: `x25519(device_static_x25519_sk, peer_device_x25519_pk)` inside an authenticated channel. The account's threshold signature then signs a `PairwiseKeyEstablished` record in the account ledger to remember which device anchors the relationship.
* From `RID_AB` derive per-relationship keys using separated identity and permission contexts:

  * `K_box` — encrypt envelopes (HPKE/XChaCha20-Poly1305) via `IdentityKeyContext::RelationshipKeys{relationship_id}` + `BoxKey` derivation.
  * `K_tag` — compute rotating **routing tags** (rtag) via `IdentityKeyContext::RelationshipKeys{relationship_id}` + `TagKey` derivation.
  * `K_psk` — PSK for mutual-auth handshake via `IdentityKeyContext::RelationshipKeys{relationship_id}` + `PskKey` derivation.
  * `K_topic` — rotating topic base (epochal label) for housekeeping.

**Session Type Integration**: Key derivation protocols use session types for compile-time safety in `crates/coordination/src/key_derivation_choreography.rs`.

**Ledger-Backed Link Device Consensus**: To avoid divergent link device selection during flaky network conditions:
1. **Initial Selection**: During handshake negotiation, both sides propose their lexicographically smallest online device ID
2. **Consensus Recording**: The first successful DH completion writes a `PairwiseKeyEstablished` event containing the chosen link device ID and timestamp
3. **Canonical Authority**: All subsequent operations reference this ledger-recorded link device, not volatile local online status
4. **Anchor Changes**: Link device can only be changed via threshold-signed "change anchor" events

**Immediate Key Distribution**: After deriving `RID_AB`, the link device writes a `PairwiseKeyEstablished` event containing:
* The chosen link device ID (canonical anchor for this relationship)
* The derived `RID_AB` encrypted to each currently enrolled device's HPKE public key
* Relationship metadata (peer account ID, establishment timestamp)
* All derived keys (`K_box`, `K_tag`, `K_psk`) encrypted per-device

**Automatic Key Rewrapping**: When new devices are added to the account:
1. **Trigger**: `DeviceAdded` events in the account ledger trigger background key rewrapping
2. **Rewrap Process**: Existing devices re-encrypt all relationship keys for the new device
3. **Update Event**: Publish `PairwiseKeyUpdate` event with keys for the new device
4. **Immediate Access**: New device can immediately participate in all existing relationships

### Key Recovery (Future Work)

**MVP Approach**: For initial development, treat lost keys as permanently broken relationships. Users can manually re-establish connections.

**Production Enhancement**: Implement automatic rekeying fallback when envelope timeouts suggest key desynchronization.

**C. Unified Journal-based State Management**

* A service each user runs that maintains a **single unified Journal CRDT** containing all account, communication, and storage state.
* **Single Source of Truth**: All state (envelopes, neighbors, manifests, quotas) managed by the Keyhive authority graph.
* **Envelope Management**: Publishes own envelopes by adding them directly to the Journal's `sbb_envelopes` map.
* **State Replication**: All state replicates via unified Journal CRDT merges with neighbors.
* **Privacy Preservation**: Does not learn sender/receiver/content due to envelope encryption.
* **Atomic Authority**: Single authority graph controls all capabilities for relay, storage, and communication permissions.

**D. Rendezvous Agent (RA)**

* One RA per relationship (logical).
* On transport change: crafts an **Offer** envelope and adds it to the local SbbDocument.
* On incoming envelopes: tries to match via `rtag` and decrypt with `K_box`, then completes a **PSK-bound** handshake (Noise/TLS/QUIC).

**E. CRDT-based Replication**

* Uses Aura's existing CRDT technology (Automerge) for bulletin board state replication.
* Membership management and broadcast achieved through CRDT operations and merges.
* Inherits fault tolerance, eventual consistency, and automatic repair from CRDT semantics.

---

## 4) Integration with Aura's Journal/Ledger System

The SBB protocol coordinates with Aura's existing journal system (`crates/journal/`) while maintaining clear separation of concerns:

### **Unified Journal State Management**

* **Aura's Journal/Ledger (`crates/journal/`)** - **Single Source of Truth for All State**
  - **Already implemented** with robust Automerge CRDT foundation
  - **Core Account State**: devices, guardians, capabilities, session epochs
  - **SBB State Integration**: `sbb_envelopes`, `sbb_neighbors`, relationship keys
  - **Storage State Integration**: `storage_manifests`, `storage_quotas`, chunk metadata
  - **Threshold signature coordination** for all critical operations across all subsystems
  - **Protocol session management** for DKD, resharing, recovery, and communication protocols
  - **Atomic State Updates**: All state changes coordinated through single CRDT with consistent visibility

### **Unified State Architecture**

```
┌─────────────────────────────────────────────────────────────┐
│              Unified Journal CRDT                           │
│            (Single Source of Truth)                         │
│                                                             │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │              Core Account State                         │ │
│ │ • devices: Map<DeviceId, DeviceInfo>                    │ │
│ │ • guardians: Map<GuardianId, GuardianInfo>              │ │
│ │ • capabilities: Map<CapabilityId, Delegation>           │ │
│ │ • revocations: Map<CapabilityId, Revocation>            │ │
│ └─────────────────────────────────────────────────────────┘ │
│                                                             │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │              SBB State (now unified)                    │ │
│ │ • sbb_envelopes: Map<Cid, SealedEnvelope>               │ │
│ │ • sbb_neighbors: Set<PeerId>                            │ │
│ │ • relationship_keys: Map<RelationshipId, Keys>          │ │
│ └─────────────────────────────────────────────────────────┘ │
│                                                             │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │              Storage State (now unified)                │ │
│ │ • storage_manifests: Map<Cid, ObjectManifest>           │ │
│ │ • storage_quotas: Map<AccountId, Quota>                 │ │
│ │ • chunk_metadata: Map<ChunkId, ChunkInfo>               │ │
│ └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### **Unified State Benefits**

* **Single Source of Truth**: All state (account, communication, storage) lives in the unified Journal CRDT managed by the Keyhive authority graph.

* **Atomic Consistency**: Capability grants/revocations for SBB relay permissions and storage access are atomically consistent - no cross-CRDT synchronization delays.

* **Simplified Logic**: When a capability is revoked, peers immediately disappear from valid neighbor views and lose storage access permissions through the unified visibility index.

* **Reduced Complexity**: Developers reason about one state model with unified update rules instead of multiple interacting CRDTs.

### Integration with Aura's Transport Abstraction

The SBB protocol fills a major gap in the current Aura architecture: how one Aura user finds and establishes a secure channel with another Aura user. The `RendezvousAgent` from this document serves as the component responsible for bootstrapping a `Connection` that conforms to Aura's `Transport` trait.

**User-facing flow:**

1. `DeviceAgent` wants to send data to Bob's account (`bob_account_id`).
2. It calls a new service, the `RendezvousManager`, with `establish_channel(bob_account_id)`.
3. The `RendezvousManager` executes the SBB protocol:
   * It crafts and gossips an "Offer" envelope.
   * It scans the SBB for a corresponding "Answer" envelope from Bob.
   * Once the Offer/Answer exchange is complete, it uses the agreed-upon transport details and the pairwise secret (`K_psk`) to establish a direct, secure QUIC or WebRTC connection.
4. This connection object, which is now a live, secure channel, is returned to the `DeviceAgent`. The agent can then use this connection with its existing `Transport` trait to send and receive application-level data.

This creates a clean separation of concerns: SBB for discovery and handshake, `Transport` trait for data transfer.

---

## 5) Protocol & Data Formats

### 4.1 Envelope (flooded object)

**Fixed-size** (e.g., 2–4 KiB), content-addressed by `cid = H(H(header_bare) || H(ciphertext))`.

**Header Structure:**

The header is composed of two parts for proper CID computation:

1. **HeaderBare** (everything except CID):
```
version: u8
epoch: u32                 # hour/day bucket
counter: u32               # per-relationship monotonic
rtag: [u8; 16]             # Trunc128( HMAC(K_tag, epoch || counter || "rt") )
ttl_epochs: u16
```

2. **Full Header** (HeaderBare + CID):
```
[HeaderBare fields above]
cid: [u8; 32]              # sha256(sha256(HeaderBare_bytes) || sha256(ciphertext))
```

**CID Computation Procedure (Merkle-like):**
1. Serialize HeaderBare using canonical CBOR encoding (sorted keys, fixed integer sizes)
2. Compute `header_hash = sha256(HeaderBare_bytes)`
3. Compute `ciphertext_hash = sha256(ciphertext)`
4. Compute `cid = sha256(header_hash || ciphertext_hash)`
5. Assemble the full header by appending the CID to HeaderBare

**Benefits**: This Merkle-like structure allows verifying header integrity without possessing the full ciphertext, enabling advanced summary-exchange protocols where headers can be gossiped independently of envelope bodies.

**Authentication Payload (proves device identity):**

```
{
  kind: "offer" | "answer" | "ack" | "rekey" | "revoke_device",
  ver: 1,
  device_cert: bytes,                 # account-TSS-signed device pubkey bundle (authentication)
  channel_binding: [u8; 32],          # H(K_psk || device_static_pub) (authentication)
  expires: u64,                       # unix time
  counter: u32,                       # mirrors header
  inner_sig: bytes                    # Sign_device( canonical_payload ) (authentication)
}
```

**Transport Offer Payload (separate from authentication):**

```
{
  transports: [ TransportDescriptor ],# Available transport options
  selected_transport?: u8,            # only in "answer"
  required_permissions: Vec<Permission>, # What permissions are needed to use transports
  capability_proof?: CapabilityToken, # Optional: proof of authorization for premium transports
}
```

**TransportDescriptor (examples):**

```
QUIC:   {"kind":"quic","addr":"203.0.113.4:6121","alpn":"hq","token":"..."}
WebRTC: {"kind":"webrtc","ufrag":"...","pwd":"...","candidates":[...]}
Tor:    {"kind":"tor","onion":"abcd.onion:443"}
BLE:    {"kind":"ble","service_uuid":"...","hint":"adv:..."}
```

### 4.2 Session-Typed CRDT Publishing & Recognition

**Session-Typed Envelope Protocol:**

```rust
// Session-typed envelope lifecycle
EnvelopeProtocol<Composing> → EnvelopeProtocol<CounterReservation> →
EnvelopeProtocol<Encrypting> → EnvelopeProtocol<Publishing> →
EnvelopeProtocol<AwaitingPropagation> → EnvelopeProtocol<Delivered>

// Integration: crates/coordination/src/envelope_choreography.rs
```

* **Type-Safe Offer flow (A→B):**

  1. A uses session-typed counter reservation protocol via the account's CRDT ledger (see Counter Coordination below).
  2. A collects current descriptors, builds payload with session type safety.
  3. AEAD-encrypt with `K_box`, compute `rtag` for `(epoch,counter)` using session-typed crypto operations.
  4. A performs CRDT operation to add envelope to local SbbDocument's `envelopes` map with type safety.
  5. A initiates CRDT merges with all neighbors using session-typed merge protocols.

**Session-Typed Counter Coordination:**

```rust
// Session-typed counter coordination protocol
CounterProtocol<ProposingIncrement> → CounterProtocol<CollectingSignatures> →
CounterProtocol<ThresholdMet> → CounterProtocol<CounterReserved>

// Integration: crates/coordination/src/counter_choreography.rs
```

Multi-device accounts require session-typed coordination to ensure unique counters across all devices:

**Session-Typed Threshold-Signed Counter Events**: Counter coordination uses session types for compile-time safety with runtime witnesses for distributed thresholds.

**Type-Safe Coordination Flow**:
1. Device uses `CounterProtocol<ProposingIncrement>` to propose counter increment
2. Session type ensures proper threshold signature collection via runtime witness `ThresholdSignaturesMet`
3. Protocol state transitions ensure only valid counter assignments
4. Session-typed retry logic handles race conditions safely
5. Counter reservation completes with protocol state `CounterProtocol<CounterReserved>`

This exercises Aura's core threshold signature machinery with session type safety while maintaining distributed coordination properties.

* **Recognition (B):**

  * During CRDT merge, B receives new envelopes in the `envelopes` map.
  * For each new envelope header, B checks recognition window `(epoch±Δ, counter±k)`:
    * If header.rtag equals `Trunc128(HMAC(K_tag, epoch'||ctr'||"rt"))`, try decrypt with `K_box`.
    * If decrypt & signature OK and not replayed: deliver to RA.

* **Answer flow (B→A):**

  * B creates envelope with `selected_transport`, adds to SbbDocument via CRDT operation.
  * CRDT merges propagate the answer back to A's network view.
  * Parties then handshake directly on selected transport.

### 4.3 Handshake (data-plane bring-up)

* Prefer **Noise IKpsk2** (fast if peer static known), fall back to **XXpsk3**.
* Or, **TLS 1.3 over QUIC** with **external PSK** (bind to `K_psk`).
* **Transcript bindings** MUST cover:

  * Both device certificates,
  * `channel_binding`,
  * Selected transport tuple (prevents unknown-key-share / downgrade).
* After handshake: start session keys / double ratchet (or rely on QUIC/TLS app keys).

---

## 5) CRDT-based SBB Architecture

### 5.1 The Unified Journal Data Structure

Each user runs a service that maintains a **single unified Journal CRDT** containing all account, communication, and storage state managed by the Keyhive authority graph.

**Unified Journal contains:**

```rust
pub struct UnifiedAccountLedger {
    // --- Core Identity/Journal State ---
    pub devices: Map<DeviceId, DeviceInfo>,
    pub guardians: Map<GuardianId, GuardianInfo>,

    // --- Keyhive Capability State ---
    pub capabilities: Map<CapabilityId, Delegation>,
    pub revocations: Map<CapabilityId, Revocation>,

    // --- SBB State (now part of the main ledger) ---
    pub sbb_envelopes: Map<Cid, SealedEnvelope>,
    pub sbb_neighbors: Set<PeerId>,
    pub relationship_keys: Map<RelationshipId, RelationshipKeys>,

    // --- Storage State ---
    pub storage_manifests: Map<Cid, ObjectManifest>,
    pub storage_quotas: Map<AccountId, Quota>,
    pub chunk_metadata: Map<ChunkId, ChunkInfo>,
}

/// Holds the derived keys for a specific pairwise relationship.
pub struct RelationshipKeys {
    pub k_box: [u8; 32], // For envelope encryption
    pub k_tag: [u8; 32], // For routing tag computation
    pub k_psk: [u8; 32], // For transport PSK
    /// The epoch at which these keys were created, serving as a GC hook.
    pub created_at_epoch: u64,
}

// Authentication information (who the peer is)
struct PeerAuthentication {
  device_id: DeviceId,
  account_id: AccountId,
  last_authenticated: u64,          // Last successful authentication
  trust_level: TrustLevel,          // Identity trust assessment
}

// Permission information (what the peer can do)
struct PeerPermissions {
  // Communication permissions
  relay_permissions: Vec<Permission>,
  communication_permissions: Vec<Permission>,

  // Storage permissions
  storage_permissions: Vec<Permission>,

  // Capability metadata
  granted_capabilities: Vec<CapabilityToken>,
  last_permission_update: u64,
}
```

Where:
* `EnvelopeState` contains the envelope's binary payload and `expires_at_epoch`
* Background processes propose remove operations for expired envelopes
* `neighbors` represents the active view (4-8 peers for frequent merges)
* `known_peers` represents the passive view (pool for neighbor replacement)

### 5.2 Mapping Gossip Concepts to CRDT Operations

**Membership Management (CRDT-based HyParView)**

1. **Joining the Network**: New node N connects to bootstrap peer B. They perform a full CRDT merge of their SbbDocuments. N gets B's neighbor list and proposes adding B to its own neighbors set.

2. **Populating Passive View**: Periodically, node A picks a random neighbor B and inspects B's neighbors set (available locally via CRDT merge). A adds unknown peers to its `known_peers` map.

3. **Handling Failures**: If node A fails to merge with neighbor B repeatedly, A removes B from its `neighbors` set and adds a random peer from `known_peers`. This propagates on next merge.

4. **Reconnection Strategy**:
   * **MVP**: Simple reconnection to all known neighbors simultaneously
   * **Future Enhancement**: Jittered reconnection to prevent thundering herd problems

**Broadcast (CRDT-based Plumtree)**

1. **Eager Push**: When Alice publishes a rendezvous envelope, her DeviceAgent adds it to the local SbbDocument's `envelopes` map, then immediately initiates CRDT merges with all active neighbors. This creates spanning-tree broadcast like Plumtree's eager push.

2. **Lazy Pull & Duplicate Suppression** (Automatic):
   * **Lazy Pull**: If a node was offline and missed the eager push, the next CRDT merge with any peer automatically identifies and delivers missing envelopes. The CRDT sync protocol *is* the lazy pull mechanism.
   * **Duplicate Suppression**: Envelopes are keyed by CID in the CRDT map. Multiple copies from different peers are automatically handled by CRDT merge semantics.

### 5.3 Web-of-Trust Security Model

**Primary Authorization Layer**: The WoT serves as the main access control mechanism:
* You only sync with accounts you trust (contacts and their contacts)
* Provides strong protection against stranger-driven Sybil attacks
* Enables social accountability and recovery mechanisms

**Limitations of Social-Only Defense**:
* **Weakest Link Problem**: Compromised trusted accounts become spam injection points
* **User Burden**: Relies on users to actively detect and prune misbehaving contacts
* **Scale Vulnerability**: Single compromised node can spam entire local neighborhood
* **No Global Cost**: Unlike PoW, no physics-based limit on spam volume

**Rate Limiting as Secondary Defense**:
* **Per-neighbor limits**: Max 1 merge per second per trusted neighbor
* **Exponential backoff**: If a neighbor exceeds limits, increase backoff exponentially
* **Social circuit breaker**: Persistent misbehavior triggers user notification and option to disconnect
* **Recovery path**: Users can leverage social recovery to help compromised contacts restore their accounts

**Critical Gap**: Without global cost-to-attack, the system remains vulnerable to high-volume spam from a small number of compromised-but-trusted nodes.

### 5.4 Architectural Risk Assessment

**Scenario: Compromised Trusted Node Attack**
1. Attacker compromises a single trusted account in the network
2. That account is trusted by N users and can relay to their neighborhoods
3. Attacker floods envelopes at maximum rate (1/sec per neighbor) across all relationships
4. Total spam rate = N messages/second with no global cost limitation
5. Individual users must manually detect and disconnect the compromised node

**Mitigations in Current Design**:
* Per-neighbor rate limiting reduces spam rate per relationship
* Exponential backoff increases penalties for sustained abuse
* Social circuit breaker enables user intervention
* CRDT TTL eventually cleans up spam envelopes

**Fundamental Limitation**:
The WoT model provides excellent authorization (who can relay) but weak accounting (how much they can relay). A determined attacker with even one compromised trusted node can generate significant spam before social mechanisms respond.

### 5.4 Benefits of Policy-Controlled CRDT Approach

* **Unified Technology**: Single replication technology (CRDTs) for both account ledger and SBB
* **Inherent Resilience**: Fault tolerance, eventual consistency, and lazy repair are built-in CRDT properties
* **Controlled Resource Usage**: Policy-based merge limits prevent resource exhaustion attacks
* **Privacy Preserving**: Neighbor topology information does not leak through document merges
* **Automatic Cleanup**: Distributed garbage collection ensures expired data is purged

**Spam Prevention**

* **Fixed-size envelopes** only; large content is out-of-band after handshake.
* **Social rate limiting**: Trust-based per-neighbor limits with exponential backoff.
* **TTL enforcement**: Automatic cleanup of expired envelopes.

**Web-of-Trust Trade-offs**:
* **Advantages**: Lower computational cost, social accountability, account recovery
* **Disadvantages**: Vulnerable to compromised trusted nodes, relies on user vigilance
* **Missing**: Global cost mechanism to limit spam volume from any single source

---

## 6) Privacy Considerations

* **Unlinkability:** No global identifiers in headers; `rtag` rotates with `(epoch,counter)`. Only holders of `K_tag` can compute valid `rtag`s for a relationship.
* **Content confidentiality:** AEAD via `K_box`. Hosts can’t read envelopes.
* **Traffic analysis:** Fixed sizes; randomized fanout; periodic pull; optional **cover envelopes** (well-formed, random ciphertext).
* **Access pattern privacy (extensions):**

  * **OHTTP** ingress for remote/roaming devices to hide IP from first hop.
  * **Bucketization:** forward by coarse `rtag` buckets to reduce selective drop.
* **Compromise recovery:** threshold-signed `rekey` or `revoke_device` envelopes to rotate device certs or retire a compromised device. Consider periodically rotating `RID_AB` via a rekey ceremony (threshold-signed notice inside the pairwise channel).

**Critical security note:** In section 5.1 Envelope, the `device_cert` and `inner_sig` are critical. The `inner_sig` is created by the device's private key, and the `device_cert` is the proof that this device key is authorized by the account's Group Public Key. This directly ties the SBB's security to Aura's core threshold identity model.

**Threats & mitigations**

* **Spam floods from compromised trusted nodes:** Social rate limiting + exponential backoff + user notification. **Limitation**: No global cost limit on spam volume.
* **Weakest link attacks:** Trust graph pruning + social recovery. **Limitation**: Requires active user intervention.
* **High-volume abuse:** Rate limiting per neighbor. **Limitation**: Single compromised node can still spam its local neighborhood.
* **Selective forwarding:** CRDT repair + redundant paths through web of trust.
* **Unknown-key-share / downgrade:** strict transcript binding across certs, PSK, and selected transport tuple.
* **Malicious SBB relay:** learns only traffic volume; cannot decrypt or link envelope pairs due to encryption.

**Note**: The current model trades cryptographic spam resistance (PoW) for social spam resistance (WoT). This improves usability but creates vulnerabilities that require future cryptographic or economic solutions.

---

## 7) Parameters (PoC defaults)

* Epoch: 1 hour; skew window Δ = 1 (check previous/next hour).
* Envelope size: 2048 bytes (2 KiB) padded (smaller without PoW field).
* Fanout: 4; TTL: 24 epochs (1 day).
* Cache: 4096 envelopes LRU.
* Rate limiting: 1 merge/sec per neighbor, exponential backoff on violations.
* Counters: monotonic per relationship, coordinated via threshold-signed `IncrementCounter` events; store `(last_seen_counter)` to prevent replay.

---

## 8) Recommended Rust Libraries

**Async/runtime**

* `tokio` — async runtime.
* `tracing` + `tracing-subscriber` — structured logs.

**Crypto**

* **KDF/HKDF/Hash:** `rust-crypto` family (`hkdf`, `sha2`) or `ring` (fast, audited).
* **AEAD:** `chacha20poly1305` (XChaCha via `chacha20poly1305` with `xchacha20poly1305` feature) or `aes-gcm`.
* **HPKE:** `hpke` (RustCrypto’s HPKE crate) for sender-static/receiver-static HPKE; or use X25519+XChaCha AEAD directly.
* **Noise:** `snow` (Noise Protocol in Rust) for IKpsk2/XXpsk3.
* **Ed25519:** `ed25519-dalek`.
* **BLS12-381 / Pairing:** `blstrs` (fast) or `bls12_381` (zkcrypto).
* **Threshold signatures (FROST):**

  * Curve25519 FROST: `frost` (Zcash Foundation’s implementation).
  * BLS FROST: check `vsss-rs`/`threshold-bls` ecosystems; for PoC you can stub TSS with multi-sig and upgrade.
* **Randomness:** `rand_core` + `getrandom`.

**Networking / Transports**

* **QUIC:** `quinn`.
* **WebRTC (optional PoC):** `webrtc` (webrtc-rs); offers data channels & ICE.
* **Tor (optional):** control via `arti-client` (Rust Tor client) or treat onion as opaque descriptor.

**Serialization & Storage**

* **Serde + CBOR:** `minicbor` or `serde_cbor` (deterministic encoding).
* **Protobuf (alternative):** `prost` if you prefer .proto schemas.
* **Content addressing:** `multihash`, `cid` (from `ipld` crates) if you want CIDs; otherwise SHA-256 digest struct.
* **KV store:** `sled` (embedded, simple) or `rocksdb` for larger caches.

**CRDT / Replication**

* **Automerge** (`automerge`) — Primary CRDT library used by Aura for both AccountLedger and SbbDocument.
* **Alternative**: `y-crdt` if you prefer a different CRDT implementation, but Automerge aligns with existing Aura architecture.

**Rate limiting & time**

* `governor` (token bucket).
* `tokio-cron-scheduler` or custom intervals.

---

## 9) Session-Typed APIs (Rust)

```rust
// Session-typed envelope protocols
EnvelopeProtocol<Composing> → EnvelopeProtocol<Publishing> → EnvelopeProtocol<Delivered>
RendezvousProtocol<OfferPhase> → RendezvousProtocol<AnswerPhase> → RendezvousProtocol<Connected>

// Envelope types with session type integration
#[derive(Serialize, Deserialize)]
pub struct Header { /* as above */ }

#[derive(Serialize, Deserialize)]
pub enum PayloadKind { Offer, Answer, Ack, Rekey, RevokeDevice }

pub struct Envelope {
    pub header: Header,
    pub ciphertext: Vec<u8>, // padded to FIXED_SIZE - header_len
    pub cid: [u8; 32],
}

// Session-typed SBB node with clean authentication/authorization separation
#[async_trait::async_trait]
pub trait SessionTypedSbbNode {
    // Communication operations with session type safety
    async fn publish_envelope<S: EnvelopeState>(
        &self,
        env: Envelope,
        device_auth: DeviceAuthentication,
        envelope_protocol: EnvelopeProtocol<S>
    ) -> Result<(CID, EnvelopeProtocol<S::NextState>)>;

    async fn subscribe_envelopes<S: SubscriptionState>(
        &self,
        filter: EnvelopeFilter,
        device_auth: DeviceAuthentication,
        subscription_protocol: SubscriptionProtocol<S>
    ) -> Result<(EnvelopeStream, SubscriptionProtocol<S::NextState>)>;

    // Session-typed CRDT operations with authenticated peers
    async fn merge_with_authenticated_peer<S: MergeState>(
        &self,
        peer_id: PeerId,
        merge_protocol: MergeProtocol<S>
    ) -> Result<MergeProtocol<S::NextState>>;

    // Event callbacks
    fn on_envelope(&self, cb: Arc<dyn Fn(&Header, &[u8]) + Send + Sync>);
    fn on_peer_authenticated(&self, cb: Arc<dyn Fn(PeerId, PeerAuthentication) + Send + Sync>);

    // State access
    fn get_sbb_document(&self) -> &SbbDocument;
    fn get_peer_authentication(&self, peer_id: PeerId) -> Option<PeerAuthentication>;
}

// Separate authorization layer
#[async_trait::async_trait]
pub trait SbbPermissionManager {
    // Permission verification
    async fn verify_peer_permissions(&self, peer_id: PeerId, required_permissions: Vec<Permission>) -> Result<bool>;
    async fn grant_permissions(&self, peer_id: PeerId, permissions: Vec<Permission>) -> Result<CapabilityToken>;

    // Peer selection based on permissions
    async fn select_storage_peers(&self, storage_requirements: StorageRequirements) -> Result<Vec<PeerId>>;
    async fn select_relay_peers(&self, communication_requirements: CommunicationRequirements) -> Result<Vec<PeerId>>;

    // Permission state
    fn get_peer_permissions(&self, peer_id: PeerId) -> Option<PeerPermissions>;
}

// Separated statistics for authentication and authorization
struct SbbAuthenticationStats {
    authenticated_peers: usize,
    authentication_attempts: u64,
    authentication_failures: u64,
    last_authentication_time: u64,
}

struct SbbPermissionStats {
    permission_verifications: u64,
    permission_grants: u64,
    permission_revocations: u64,
    active_capability_tokens: usize,
}

// Session-typed Rendezvous Manager with separated authentication and authorization
pub struct SessionTypedRendezvousManager {
    /* holds authenticated relationships to other accounts */
    /* integrates with session-typed authentication and permission layers */
}
impl SessionTypedRendezvousManager {
    // Establish authenticated channel with session type safety
    pub async fn establish_authenticated_channel<S: RendezvousState>(
        &mut self,
        account_id: AccountId,
        rendezvous_protocol: RendezvousProtocol<S>
    ) -> Result<(Box<dyn AuthenticatedTransport>, RendezvousProtocol<S::NextState>)>;

    // Get authenticated peers using session types
    pub async fn get_authenticated_peers<S: PeerDiscoveryState>(
        &self,
        account_id: AccountId,
        discovery_protocol: PeerDiscoveryProtocol<S>
    ) -> Result<(Vec<PeerId>, PeerDiscoveryProtocol<S::NextState>)>;

    // Session-typed permission management
    pub async fn establish_permissions<S: PermissionState>(
        &mut self,
        peer_id: PeerId,
        permissions: Vec<Permission>,
        permission_protocol: PermissionProtocol<S>
    ) -> Result<(CapabilityToken, PermissionProtocol<S::NextState>)>;

    // Integration: Uses session types from crates/coordination/src/rendezvous_choreography.rs
}

// Rendezvous agent (per relationship)
pub struct RendezvousAgent { /* holds K_box, K_tag, K_psk, counters */ }
impl RendezvousAgent {
    pub async fn publish_offer(&mut self, transports: Vec<TransportDescriptor>);
    pub fn handle_header(&mut self, hdr: &Header, ciphertext: &[u8]);
}
```

---

## 10) MVP Development Plan (2–4 week sprint)

**Milestone A — Multi-device foundation (days 1–7)**

* Multi-device X25519 DH with deterministic link device selection using session types
* HPKE-encrypted key distribution via account ledger events with session-typed protocols
* Threshold-signed `IncrementCounter` coordination for envelope counters using `CounterProtocol<T>` session types
* Basic envelope struct + CBOR serialization with session-typed envelope lifecycle
* **Core Focus**: Exercise threshold signature machinery and multi-device key sharing with compile-time protocol safety

**Milestone B — Multi-device SBB (days 6–10)**

* Two multi-device accounts (2 devices each) exchanging Offer/Answer using session-typed `EnvelopeProtocol<T>`
* Counter coordination across devices using threshold signatures with session type safety
* Key distribution verification (all devices can decrypt relationship keys) via session-typed key protocols
* In-memory CRDT with basic envelope recognition using session-typed merge protocols
* **Core Focus**: Prove multi-device architecture works end-to-end with compile-time protocol verification

**Milestone C — Network SBB (days 9–16)**

* Multi-device accounts over QUIC network (4 accounts, 2 devices each) with session-typed transport protocols
* Distributed threshold signature coordination using session-typed choreographic protocols
* CRDT merge across actual network boundaries with session type safety
* Envelope propagation and recognition across device mesh using session-typed network protocols
* **Core Focus**: Distributed multi-device coordination at scale with compile-time safety guarantees

**Milestone D — Complete data plane (days 14–20)**

* Full rendezvous → PSK handshake → data transfer using session-typed `RendezvousProtocol<T>` transitions
* Any device from Account A can establish channel to any device from Account B with session type safety
* Integration test: multi-device mesh sending data via session-typed protocols
* **Core Focus**: Complete multi-device rendezvous flow with compile-time protocol verification

**Milestone E — Production hardening (days 18–24)**

* Social rate limiting with exponential backoff using session-typed rate limit protocols
* Robust threshold signature retry logic with session type safety
* Key distribution edge cases (device addition during key establishment) handled via session-typed recovery protocols
* Integration tests and CLI demo using session-typed choreographic protocols
* **Core Focus**: Handle multi-device edge cases robustly with compile-time safety

**MVP Deliverables**: Working multi-device rendezvous demonstrating Aura's core threshold identity value proposition with integrated storage trust bootstrapping

**Post-MVP Roadmap**: Now focused on scaling and advanced features rather than architectural fundamentals

## 10.1 Storage Integration Benefits

**Unified System Advantages**:
1. **Bootstrapped Trust**: Communication relationships immediately establish storage trust without separate onboarding
2. **Shared Infrastructure**: Single peer discovery, capability management, and transport layer serves both use cases
3. **Consistent Social Model**: Web-of-trust principles apply uniformly to communication relay and storage provider selection
4. **Reduced Attack Surface**: Single capability verification system eliminates inconsistencies between subsystems
5. **Graceful Degradation**: Communication failures don't affect storage access and vice versa

**Implementation Synergies**:
- Communication channels can announce available storage capacity
- Storage relationships can bootstrap communication trust for new peers
- Unified CRDT replication reduces bandwidth overhead
- Shared connection management improves resource efficiency
- Single trust evaluation algorithm for all peer interactions

**Enhanced User Experience**:
- Seamless progression from communication to storage sharing
- Unified permission management across all peer interactions
- Consistent trust indicators for storage and communication reliability
- Single interface for managing both communication and storage relationships

---

## 11) Testing & Verification

* **Deterministic vectors:** encode several envelopes with fixed seeds; verify `cid` stability and padding.
* **Adversarial tests:** replay old counters; mutate header; invalid PoW; invalid inner_sig; ensure rejection.
* **CRDT propagation tests:** spawn 50 SBB nodes (Tokio tasks), random CRDT merge topology, simulate network partitions; ensure Offer reaches peer with high probability via eventual CRDT consistency.
* **Privacy sanity:** ensure third-party node cannot distinguish target of any passing envelope beyond traffic volume.

---

## 12) Architectural Decision: Unified State Model

### **Key Change from Original Design**

The original design maintained separate CRDT documents for different subsystems:
- Journal CRDT for account state
- SbbDocument CRDT for communication state
- Storage Index for storage state

**Problem**: This created synchronization complexity, potential inconsistencies, and cognitive overhead as described in feedback.

### **New Unified Approach**

All state is now managed by a **single unified Journal CRDT** controlled by the Keyhive authority graph:

```rust
pub struct UnifiedAccountLedger {
    // Core Identity/Journal State
    pub devices: Map<DeviceId, DeviceInfo>,
    pub guardians: Map<GuardianId, GuardianInfo>,

    // Keyhive Capability State
    pub capabilities: Map<CapabilityId, Delegation>,
    pub revocations: Map<CapabilityId, Revocation>,

    // SBB State (now part of main ledger)
    pub sbb_envelopes: Map<Cid, SealedEnvelope>,
    pub sbb_neighbors: Set<PeerId>,
    pub relationship_keys: Map<RelationshipId, RelationshipKeys>,

    // Storage State
    pub storage_manifests: Map<Cid, ObjectManifest>,
    pub storage_quotas: Map<AccountId, Quota>,
}
```

### **Benefits of Unification**

1. **Single Source of Truth**: Capability revocations immediately affect both SBB relay permissions and storage access
2. **Atomic Consistency**: No cross-CRDT synchronization delays or race conditions
3. **Simplified Logic**: Keyhive visibility index controls materialization across all subsystems
4. **Reduced Complexity**: One state model with unified update rules instead of multiple interacting CRDTs

### **Implementation Strategy**

- Extend existing Journal CRDT rather than creating separate documents
- Use Keyhive's visibility index for unified access control
- Maintain capability-driven garbage collection for ephemeral data like envelopes
- Preserve existing CRDT conflict resolution semantics

This architectural change addresses the core feedback about truly unifying the state management rather than just sharing infrastructure.

### 12.1 Planning for Garbage Collection

To facilitate future garbage collection of ephemeral SBB data, the following hooks are included in the design:

1.  **Envelope Metadata**: Every envelope header contains an `epoch` and a `ttl_epochs`. This allows a simple, local GC process to prune any envelope whose TTL has expired (`current_epoch > envelope.epoch + envelope.ttl_epochs`).

2.  **Key Rotation Epochs**: The `RelationshipKeys` stored in the unified journal are tagged with a `created_at_epoch`. When a periodic key rotation occurs, a new `RelationshipKeys` entry is created with the new epoch. This provides a powerful cryptographic signal for GC.

A future GC process can prune any envelope where `envelope.epoch < relationship.keys.created_at_epoch`, because that envelope is encrypted with an obsolete key. This is more efficient and secure than relying on TTLs alone, as it allows for bulk pruning of all old envelopes for a relationship in a single, verifiable action.

## 13) Post-MVP Roadmap

**Phase 1 - Advanced Multi-Device Features**:
* Dynamic device addition/removal during active relationships
* Key rotation and recovery mechanisms
* Cross-device envelope acknowledgment protocols

**Phase 2 - Spam Defense Enhancement**:
* **Capability Tokens**: Cryptographic "relay credits" (implemented with keyhive authorization tokens) consumed per envelope
* **Economic Costs**: Per-envelope micropayments or stake-based publishing rights
* **Sophisticated Reputation**: Automated scoring and misbehavior detection
* **Adaptive Rate Limiting**: Dynamic thresholds based on node behavior
* **Alternative Global Costs**: Research replacements for PoW that provide physics-based spam resistance

**Phase 3 - Scale & Performance**:
* Jittered reconnection for thundering herd prevention
* Selective CRDT sync with delta-state optimization
* Advanced membership protocols (HyParView/Plumtree)
* CRDT compaction strategies

**Phase 4 - Privacy & Anonymity**:
* OHTTP front-end for IP hiding
* Bucketization by truncated rtag
* Private membership management
* Cover traffic and timing obfuscation

**Phase 5 - Advanced Features**:
* Multiple transport support (WebRTC, Tor, BLE)
* Threshold Diffie-Hellman ceremonies
* Attested resource delegation (Biscuit/Macaroon)
* Formal verification of protocol properties
