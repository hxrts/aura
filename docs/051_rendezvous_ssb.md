# Rendezvous & Social Bulletin Board (SBB)

**Transport-agnostic private reconnection over a web of trust**

> **Note:** This document has been updated to align with the existing Aura architecture (080_...). The SBB protocol fills a major gap in the current Aura design: how one Aura user finds and establishes a secure channel with another Aura user. The core identity concepts are so aligned that integration is not only possible but highly desirable.

---

## 1) Motivation

Aura accounts are **threshold identities** (account-level) with many devices underneath. After two accounts connect once, they should be able to:

* go offline, change networks/devices/transports,
* and still **find each other privately** and re-establish a channel,
* **without** revealing their social graph or stable identifiers to any relays.

We achieve this by flooding small, **sealed, fixed-size envelopes** through a **social bulletin board (SBB)** that’s hosted by your contacts (and optionally their contacts) rather than by a central relay. Only the intended counterparty can recognize and decrypt an envelope; everyone else stores/forwards it blindly with quotas.

---

## 2) Goals & Non-Goals

**Goals**

* Transport-agnostic rendezvous (QUIC, WebRTC, Tor, BLE…).
* Privacy: no long-lived global IDs in discovery; pairwise unlinkability.
* Offline/partition tolerance via store-and-forward.
* **Core Integration**: Deeply integrated with Aura's threshold identity model and multi-device architecture.
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
* From `RID_AB` derive per-relationship keys:

  * `K_box` — encrypt envelopes (HPKE/XChaCha20-Poly1305).
  * `K_tag` — compute rotating **routing tags** (rtag) to let only B recognize A→B envelopes.
  * `K_psk` — PSK for the mutual-auth handshake on any transport.
  * `K_topic` — rotating topic base (epochal label) for housekeeping.

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

**C. CRDT-based Social Bulletin Board (SBB)**

* A service each user runs that maintains its own **SbbDocument** (an Automerge CRDT document).
* **Publishes** own envelopes by adding them to the local SbbDocument's `envelopes` map.
* **Replicates** the bulletin board state via CRDT merges with neighbors.
* **Does not learn** sender/receiver/content due to envelope encryption.

**D. Rendezvous Agent (RA)**

* One RA per relationship (logical).
* On transport change: crafts an **Offer** envelope and adds it to the local SbbDocument.
* On incoming envelopes: tries to match via `rtag` and decrypt with `K_box`, then completes a **PSK-bound** handshake (Noise/TLS/QUIC).

**E. CRDT-based Replication**

* Uses Aura's existing CRDT technology (Automerge) for bulletin board state replication.
* Membership management and broadcast achieved through CRDT operations and merges.
* Inherits fault tolerance, eventual consistency, and automatic repair from CRDT semantics.

---

## 4) Integration with the Aura CRDT Ledger

The SBB protocol is the rendezvous and channel establishment layer for communication between different Aura accounts. It is distinct from the intra-account CRDT ledger, which is used to maintain state consistency between devices owned by a single account. An Aura agent will use the CRDT to coordinate its own devices and the SBB to discover and connect to other users.

### Architectural Role: Intra-Account vs. Inter-Account

* **Aura's CRDT Ledger** is for **Intra-Account synchronization**. It keeps Alice's phone and Alice's laptop in a consistent state. It is the source of truth for her devices, guardians, and policies. All of Alice's devices have full access to this ledger.

* **The SBB Protocol** is for **Inter-Account rendezvous**. It's how Alice (as a unified threshold identity) finds Bob (another unified threshold identity) across the internet without a central server.

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

**Ciphertext (AEAD sealed with K_box; CBOR/Protobuf inside):**

```
{
  kind: "offer" | "answer" | "ack" | "rekey" | "revoke_device",
  ver: 1,
  device_cert: bytes,                 # account-TSS-signed device pubkey bundle
  channel_binding: [u8; 32],          # H(K_psk || device_static_pub)
  transports: [ TransportDescriptor ],# opaque: quic/webrtc/tor/ble...
  selected_transport?: u8,            # only in "answer"
  expires: u64,                       # unix time
  counter: u32,                       # mirrors header
  inner_sig: bytes                    # Sign_device( canonical_payload )
}
```

**TransportDescriptor (examples):**

```
QUIC:   {"kind":"quic","addr":"203.0.113.4:6121","alpn":"hq","token":"..."}
WebRTC: {"kind":"webrtc","ufrag":"...","pwd":"...","candidates":[...]}
Tor:    {"kind":"tor","onion":"abcd.onion:443"}
BLE:    {"kind":"ble","service_uuid":"...","hint":"adv:..."}
```

### 4.2 CRDT-based Publishing & Recognition

* **Offer flow (A→B):**

  1. A reserves the next `counter` via the account's CRDT ledger (see Counter Coordination below).
  2. A collects current descriptors, builds payload.
  3. AEAD-encrypt with `K_box`, compute `rtag` for `(epoch,counter)`, add PoW.
  4. A performs CRDT operation to add envelope to local SbbDocument's `envelopes` map.
  5. A initiates CRDT merges with all neighbors in its active view.

**Counter Coordination:**

Multi-device accounts require coordination to ensure unique counters across all devices in the account:

**Threshold-Signed Counter Events**: Before publishing an envelope, a device writes a threshold-signed `IncrementCounter{relationship_id, epoch}` event to the account ledger. Only one such event per relationship/epoch can be accepted by the CRDT state machine.

**Coordination Flow**:
1. Device wants to publish envelope for relationship R in epoch E
2. Device proposes `IncrementCounter{R, E}` event with threshold signature
3. If successful, device gets unique counter value for that relationship/epoch
4. If another device raced and won, retry with next epoch or backoff
5. Use assigned counter to publish envelope

This exercises Aura's core threshold signature machinery and ensures proper multi-device coordination.

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

### 5.1 The SbbDocument Data Structure

Each user runs an SBB service that maintains its own **SbbDocument** - an Automerge CRDT document separate from their personal AccountLedger. This document represents the node's view of the bulletin board state, envelopes, and neighbors.

**SbbDocument contains:**

```rust
{
  // Rendezvous envelopes currently "pinned" to the bulletin board
  envelopes: ObservedRemoveMap<CID, EnvelopeState>,
  
  // Active neighbors (small set for frequent CRDT merges)
  neighbors: AddWinsSet<PeerId>,
  
  // Passive view (larger pool of known peers)
  known_peers: ObservedRemoveMap<PeerId, LastSeenEpoch>
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

## 9) Minimal APIs (Rust)

```rust
// Envelope types
#[derive(Serialize, Deserialize)]
pub struct Header { /* as above */ }

#[derive(Serialize, Deserialize)]
pub enum PayloadKind { Offer, Answer, Ack, Rekey, RevokeDevice }

#[derive(Serialize, Deserialize)]
pub struct Payload { /* as above */ }

pub struct Envelope {
    pub header: Header,
    pub ciphertext: Vec<u8>, // padded to FIXED_SIZE - header_len
    pub cid: [u8; 32],
}

// CRDT-based SBB node
#[async_trait::async_trait]
pub trait SbbNode {
    // Add envelope to local SbbDocument CRDT
    async fn publish_envelope(&self, env: Envelope) -> Result<CID>;
    
    // Trigger CRDT merge with specific peer
    async fn merge_with_peer(&self, peer_id: PeerId) -> Result<()>;
    
    // Trigger CRDT merge with all active neighbors
    async fn merge_with_neighbors(&self) -> Result<()>;
    
    // Set callback for new envelopes discovered during CRDT merges
    fn on_envelope(&self, cb: Arc<dyn Fn(&Header, &[u8]) + Send + Sync>);
    
    // Access local SbbDocument state
    fn get_document(&self) -> &SbbDocument;
    
    fn stats(&self) -> SbbStats;
}

// Rendezvous Manager - integrates with DeviceAgent and Transport
pub struct RendezvousManager { /* holds relationships to other accounts */ }
impl RendezvousManager {
    pub async fn establish_channel(&mut self, account_id: AccountId) -> Result<Box<dyn Transport>>;
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

* Multi-device X25519 DH with deterministic link device selection
* HPKE-encrypted key distribution via account ledger events
* Threshold-signed `IncrementCounter` coordination for envelope counters
* Basic envelope struct + CBOR serialization
* **Core Focus**: Exercise threshold signature machinery and multi-device key sharing

**Milestone B — Multi-device SBB (days 6–10)**

* Two multi-device accounts (2 devices each) exchanging Offer/Answer
* Counter coordination across devices using threshold signatures
* Key distribution verification (all devices can decrypt relationship keys)
* In-memory CRDT with basic envelope recognition
* **Core Focus**: Prove multi-device architecture works end-to-end

**Milestone C — Network SBB (days 9–16)**

* Multi-device accounts over QUIC network (4 accounts, 2 devices each)
* Distributed threshold signature coordination
* CRDT merge across actual network boundaries
* Envelope propagation and recognition across device mesh
* **Core Focus**: Distributed multi-device coordination at scale

**Milestone D — Complete data plane (days 14–20)**

* Full rendezvous → PSK handshake → data transfer
* Any device from Account A can establish channel to any device from Account B
* Integration test: multi-device mesh sending data
* **Core Focus**: Complete multi-device rendezvous flow

**Milestone E — Production hardening (days 18–24)**

* Social rate limiting with exponential backoff
* Robust threshold signature retry logic
* Key distribution edge cases (device addition during key establishment)
* Integration tests and CLI demo
* **Core Focus**: Handle multi-device edge cases robustly

**MVP Deliverables**: Working multi-device rendezvous demonstrating Aura's core threshold identity value proposition

**Post-MVP Roadmap**: Now focused on scaling and advanced features rather than architectural fundamentals

---

## 11) Testing & Verification

* **Deterministic vectors:** encode several envelopes with fixed seeds; verify `cid` stability and padding.
* **Adversarial tests:** replay old counters; mutate header; invalid PoW; invalid inner_sig; ensure rejection.
* **CRDT propagation tests:** spawn 50 SBB nodes (Tokio tasks), random CRDT merge topology, simulate network partitions; ensure Offer reaches peer with high probability via eventual CRDT consistency.
* **Privacy sanity:** ensure third-party node cannot distinguish target of any passing envelope beyond traffic volume.

---

## 12) Post-MVP Roadmap

**Phase 1 - Advanced Multi-Device Features**:
* Dynamic device addition/removal during active relationships
* Key rotation and recovery mechanisms
* Cross-device envelope acknowledgment protocols

**Phase 2 - Spam Defense Enhancement**:
* **Capability Tokens**: Cryptographic "relay credits" (e.g., Biscuits) consumed per envelope
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

---

If you want, I can spin up a starter repo layout (workspace crates: `sbb`, `rendezvous`, `crypto`, `transports-quic`, `transports-webrtc`, `cli-demo`) and stub the types and integration tests so your team can start filling in the details.
