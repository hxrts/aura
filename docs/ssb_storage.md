# Unified SSB + Storage Implementation Plan

**Status:** Active Implementation Roadmap  
**Version:** 1.0  
**References:** [`docs/040_storage.md`](../docs/040_storage.md), [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md)

---

## Core Principles

> **CODE QUALITY MANDATE**: We want concise, clean, elegant code. Zero backwards compatibility code, zero migration code, zero legacy code. Every line serves a clear purpose with no cruft.

This document provides a phased implementation plan for the integrated Social Bulletin Board (SSB) and Storage systems. These systems use a **single unified state model**:

- **Unified Journal CRDT** containing all state (account, communication, storage)
- **Single source of truth** managed by the Keyhive authority graph  
- **Atomic consistency** for all capability grants/revocations across subsystems
- **Shared infrastructure** for peer discovery, transport, and session types
- **Consistent authentication/authorization separation** across all operations

**Architecture Reference**: See [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 4 "Integration with the Aura CRDT Ledger" for the architectural rationale behind this unified approach.

---

## Phase 0: Foundation (Week 1-2)

**Goal**: Establish the unified infrastructure that both SSB and Storage will build upon.

> **REMINDER**: Clean, elegant code only. No temporary hacks, no backwards compatibility layers. If you're writing "TODO: clean this up later", stop and clean it up now.

### 0.1 Unified Capability System

**Context**: Both SSB relay permissions and storage access control use the same capability framework from [`docs/040_storage.md`](../docs/040_storage.md) Section 2.1 "DeviceAuthentication" and "Permission" types.

**Tasks**:
- [ ] Implement `Permission` enum with three scopes (Storage, Communication, Relay)
- [ ] Implement `CapabilityToken` structure with device authentication + granted permissions
- [ ] Create `DeviceAuthentication` struct separating identity proof from permissions
- [ ] Build capability verification functions with clear authentication/authorization separation

**Implementation Location**: `crates/journal/src/capability/types.rs`

**Success Criteria**:
- Can create and verify capability tokens for all three permission types
- Clear separation between "who you are" (authentication) and "what you can do" (authorization)
- Zero dependencies on any legacy policy systems
- All capability types serialize cleanly to CBOR

**Code Quality Check**:
- [ ] Each type has a single, clear responsibility
- [ ] No optional fields that should be required
- [ ] No "compatibility modes" or feature flags
- [ ] Documentation explains the separation of concerns

---

### 0.2 Separated Key Derivation

**Context**: Per [`docs/040_storage.md`](../docs/040_storage.md) Section 2.1 "KeyDerivationSpec", we separate identity-based keys from permission-based keys to enable independent rotation.

**Tasks**:
- [ ] Implement `IdentityKeyContext` enum (DeviceEncryption, RelationshipKeys, AccountRoot)
- [ ] Implement `PermissionKeyContext` enum (StorageAccess, CommunicationScope, RelayPermission)
- [ ] Create `KeyDerivationSpec` combining both contexts with clear separation
- [ ] Build key derivation functions using existing `aura-crypto` primitives

**Implementation Location**: `crates/crypto/src/key_derivation.rs`

**Success Criteria**:
- Identity keys derive independently from permission keys
- Each context type maps to specific HKDF info strings
- Key rotation in one subsystem doesn't affect others
- Clean integration with existing Effects for deterministic testing

> **REMINDER**: This is a clean slate implementation. Don't import any legacy key derivation code. Write it fresh, concise, and elegant.

**Reference**: See [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 3B "Pairwise Relationship" for relationship key derivation (K_box, K_tag, K_psk) which uses `IdentityKeyContext::RelationshipKeys`.

---

### 0.3 Unified Transport Interface

**Context**: Both systems need authenticated channels with the same peer. From [`docs/040_storage.md`](../docs/040_storage.md) Section 5 "Unified Transport Architecture".

**Tasks**:
- [ ] Define `AuthenticatedTransport` trait with device authentication methods
- [ ] Implement `establish_authenticated_channel` for QUIC connections
- [ ] Add connection pooling to reuse authenticated channels across use cases
- [ ] Create transport-level authentication (no authorization decisions)

**Implementation Location**: `crates/transport/src/unified_transport.rs`

**Success Criteria**:
- Single connection can serve both chunk transfers and envelope flooding
- Transport only verifies device signatures (authentication)
- Application layer handles all authorization decisions
- Connection pool efficiently manages peer connections

**Code Quality Check**:
- [ ] Transport trait has minimal, orthogonal methods
- [ ] No authorization logic in transport layer
- [ ] Clear error types that distinguish authentication from authorization failures
- [ ] Zero configuration modes or compatibility switches

---

## Phase 1: SSB Core (Week 3-4)

**Goal**: Implement the Social Bulletin Board for multi-device rendezvous, proving threshold signature coordination.

> **CODE QUALITY MANDATE**: Every envelope operation, every CRDT merge, every counter increment should be clean and obvious. No clever tricks, no premature optimization. Elegant simplicity.

### 1.1 Multi-Device Relationship Keys

**Context**: From [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 3B "Pairwise Relationship" - devices within an account must coordinate on link device selection and distribute relationship keys.

**Tasks**:
- [ ] Implement link device selection via ledger consensus (lexicographically smallest online device)
- [ ] Create `PairwiseKeyEstablished` event with HPKE-encrypted keys for all devices
- [ ] Build automatic key rewrapping on `DeviceAdded` events
- [ ] Derive relationship keys: K_box (encryption), K_tag (routing), K_psk (handshake)

**Implementation Location**: `crates/agent/src/relationship_keys.rs`

**Success Criteria**:
- All devices in an account can decrypt relationship keys from ledger
- Link device selection is deterministic and consensus-based
- Key distribution works even when some devices are offline
- New devices automatically receive keys for existing relationships

**Reference**: This exercises Aura's threshold signature machinery. See [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 4.2 for threshold-signed counter coordination which uses similar patterns.

> **REMINDER**: Zero backwards compatibility. This is the only key distribution mechanism. Write it right the first time.

---

### 1.2 Unified Journal State Integration

**Context**: All SSB state integrates directly into the main Journal CRDT managed by the Keyhive authority graph, eliminating separate CRDT documents as described in the revised [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 5.1.

**Tasks**:
- [ ] Extend existing Journal CRDT with `sbb_envelopes: Map<Cid, SealedEnvelope>`
- [ ] Add `sbb_neighbors: Set<PeerId>` to unified state model  
- [ ] Add `relationship_keys: Map<RelationshipId, RelationshipKeys>` to Journal
- [ ] Integrate envelope TTL handling with Journal's existing event processing

**Implementation Location**: `crates/journal/src/ledger.rs` (extend existing UnifiedAccountLedger)

**Success Criteria**:
- Single CRDT contains all account, SBB, and storage state
- Capability revocations immediately affect SBB neighbor visibility
- No cross-CRDT synchronization complexity
- Envelope expiration handled through unified event processing

**Code Quality Check**:
- [ ] Schema is minimal - no optional fields that are always present
- [ ] Authentication and authorization are clearly separated types
- [ ] No "version" fields or migration logic
- [ ] Clean unified Journal integration without wrapper bloat
- [ ] All state includes appropriate GC hooks (epoch tags, etc.)

---

### 1.3 Envelope Structure and CID Computation

**Context**: From [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 5 "Protocol & Data Formats", envelopes are fixed-size with Merkle-like CID computation.

**Tasks**:
- [ ] Implement `HeaderBare` struct (version, epoch, counter, rtag, ttl_epochs)
- [ ] Create `Header` struct (HeaderBare + CID)
- [ ] Build Merkle-like CID computation: `sha256(sha256(HeaderBare) || sha256(ciphertext))`
- [ ] Implement `Envelope` struct with header, ciphertext, padding to fixed size (2048 bytes)

**Implementation Location**: `crates/transport/src/envelope.rs`

**Success Criteria**:
- CID computation is deterministic for same inputs
- Envelope serialization always produces exactly 2048 bytes
- Can verify header integrity without full ciphertext
- CBOR serialization uses canonical encoding (sorted keys, fixed integer sizes)

> **REMINDER**: This is a clean implementation. No migration from old envelope formats, no version negotiation, no fallback modes. One format, done right.

**Reference**: See [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 4.1 for the complete envelope specification including authentication payload structure.

---

### 1.4 Threshold-Signed Counter Coordination

**Context**: Multi-device accounts need coordinated counter increments for unique envelope identifiers. From [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 4.2 "CRDT-based Publishing & Recognition".

**Tasks**:
- [ ] Create `IncrementCounter` event type for counter reservations
- [ ] Implement threshold signature collection for counter events
- [ ] Build counter reservation protocol with retry on conflicts
- [ ] Store `(relationship_id, last_seen_counter)` in account ledger

**Implementation Location**: `crates/coordination/src/choreography/counter.rs`

**Success Criteria**:
- Any device can propose counter increment
- Threshold signatures prevent unauthorized increments
- Race conditions handled cleanly with retry logic
- Counter state persists in ledger for replay protection

**Code Quality Check**:
- [ ] Counter protocol is a pure choreography (no side effects)
- [ ] Retry logic is clean and bounded (no infinite loops)
- [ ] Error cases explicitly handled, not silently ignored
- [ ] Deterministic testing via injected effects

---

## Phase 2: SSB Operations (Week 5-6)

**Goal**: Enable envelope publishing, recognition, and CRDT-based gossip.

> **CODE QUALITY MANDATE**: Gossip is complex. Fight complexity with clarity. Every function should fit on one screen. Every data structure should be immediately understandable.

### 2.1 Envelope Publishing

**Context**: From [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 4.2, publishing involves counter reservation, encryption, and CRDT addition.

**Tasks**:
- [ ] Implement `publish_envelope` coordinating counter reservation → encryption → CRDT addition
- [ ] Create routing tag computation: `Trunc128(HMAC(K_tag, epoch || counter || "rt"))`
- [ ] Build envelope encryption using K_box and XChaCha20-Poly1305
- [ ] Add published envelope to local SbbDocument's envelopes map

**Implementation Location**: `crates/transport/src/sbb_publisher.rs`

**Success Criteria**:
- Publishing fails cleanly if counter reservation fails
- Routing tags are collision-resistant within recognition window
- Encrypted envelopes are exactly 2048 bytes
- CRDT addition is atomic with envelope creation

**Reference**: This integrates with capability tokens from Phase 0. The device authentication proves publishing authority.

> **REMINDER**: No "compatibility mode" for different envelope sizes. 2048 bytes, period. Clean and simple.

---

### 2.2 Envelope Recognition

**Context**: Per [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 4.2, recognition checks routing tags in a time window and attempts decryption.

**Tasks**:
- [ ] Implement recognition window logic `(epoch±Δ, counter±k)`
- [ ] Build rtag comparison for all active relationships
- [ ] Create decryption attempt with authentication verification
- [ ] Check for replay using stored `last_seen_counter`

**Implementation Location**: `crates/transport/src/sbb_recognizer.rs`

**Success Criteria**:
- Recognition is constant time in number of envelopes (indexed by epoch)
- Decryption failures don't reveal which relationship was attempted
- Replay detection prevents old envelope reuse
- Recognition window adapts to clock skew gracefully

**Code Quality Check**:
- [ ] Recognition logic is a pure function (no side effects)
- [ ] Performance is O(relationships) not O(envelopes)
- [ ] Error handling distinguishes "not for me" from "corrupted"
- [ ] Zero clever optimizations that obscure logic

---

### 2.3 CRDT-Based Gossip

**Context**: From [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 5.2 "Mapping Gossip Concepts to CRDT Operations", we use Automerge merges for broadcast.

**Tasks**:
- [ ] Implement eager push: CRDT merge with all active neighbors on publish
- [ ] Build lazy pull: automatic sync of missing envelopes on next merge
- [ ] Create neighbor management using AddWinsSet CRDT semantics
- [ ] Add rate limiting: max 1 merge/sec per neighbor with exponential backoff

**Implementation Location**: `crates/transport/src/sbb_gossip.rs`

**Success Criteria**:
- Published envelopes reach all reachable neighbors within seconds
- Offline nodes catch up automatically on reconnection
- Duplicate envelopes suppressed by CID-based CRDT map
- Rate limiting prevents resource exhaustion from compromised peers

> **REMINDER**: CRDT semantics give us eventual consistency for free. Don't fight it with complex synchronization. Let the CRDT do its job.

**Reference**: This leverages the unified transport from Phase 0 for authenticated channels. All merges happen over authenticated connections.

---

### 2.4 Offer/Answer Choreography

**Context**: Per [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Section 4.3 "Handshake", the rendezvous protocol is Offer → Answer → Direct Handshake.

**Tasks**:
- [ ] Implement Offer envelope creation with available transports
- [ ] Build Answer envelope with selected transport
- [ ] Create PSK-bound handshake using K_psk (Noise IKpsk2 or TLS 1.3 with external PSK)
- [ ] Add transcript binding covering device certs, channel_binding, and transport tuple

**Implementation Location**: `crates/coordination/src/choreography/rendezvous.rs`

**Success Criteria**:
- Offer/Answer exchange completes even with offline devices
- Any device from Account A can connect to any device from Account B
- PSK binding prevents unknown-key-share attacks
- Transcript binding prevents downgrade attacks

**Code Quality Check**:
- [ ] Choreography is a linear async function (no state machine boilerplate)
- [ ] Error cases explicitly handled with clear error types
- [ ] Timeout logic is clean and bounded
- [ ] Handshake completion is atomic (no partial states)

---

## Phase 3: Storage Core (Week 7-8)

**Goal**: Implement capability-based storage with encrypted chunks and static replica lists.

> **CODE QUALITY MANDATE**: Storage is critical. Every byte matters. Write code that respects the data it manages. Clean encryption, clean chunking, clean manifests.

### 3.1 Object Manifest Structure

**Context**: From [`docs/040_storage.md`](../docs/040_storage.md) Section 2.1 "ObjectManifest", manifests are capability-controlled metadata with separated key derivation.

**Tasks**:
- [ ] Implement `ObjectManifest` struct with all required fields
- [ ] Create `ChunkingParams` defining chunk boundaries (1-4 MiB client-determined)
- [ ] Build `StaticReplicationHint` with target peers and fallback policy
- [ ] Add `AccessControl::CapabilityBased` with required permissions

**Implementation Location**: `crates/store/src/manifest.rs`

**Success Criteria**:
- Manifests serialize to deterministic CBOR (sorted keys)
- All fields are required (no Option<T> for mandatory data)
- Access control explicitly lists required capabilities
- Replication hints work offline (no dependency on SBB availability)

> **REMINDER**: No optional fields that are "temporarily optional" until we implement them. If it's required, make it required now. Add truly optional features later.

**Reference**: Manifests integrate with Phase 0 capability system. Storage permissions defined in `Permission::Storage` control access.

---

### 3.2 Chunk Encryption and Local Storage

**Context**: Per [`docs/040_storage.md`](../docs/040_storage.md) Section 3 "API Surface", chunks are encrypted using device-derived keys.

**Tasks**:
- [ ] Implement content chunking (client determines 1-4 MiB boundaries)
- [ ] Create chunk encryption using `KeyDerivationSpec::identity_context = DeviceEncryption`
- [ ] Build local chunk storage in `redb` database at `/chunks/<cid>/<chunk_id>`
- [ ] Add manifest storage at `/manifests/<cid>`

**Implementation Location**: `crates/store/src/chunk_store.rs`

**Success Criteria**:
- Chunks encrypt/decrypt without loss using AES-GCM or XChaCha20-Poly1305
- Content-addressing via BLAKE3 for chunk IDs
- Local storage indexes support O(1) lookup by CID
- Storage format is future-proof (clean migration if DB changes)

**Code Quality Check**:
- [ ] Encryption code is minimal (delegate to aura-crypto)
- [ ] No hand-rolled crypto primitives
- [ ] Error handling distinguishes corruption from missing chunks
- [ ] Storage operations are transactional where needed

---

### 3.3 Basic Replication to Static Peers

**Context**: From [`docs/040_storage.md`](../docs/040_storage.md) Section 8 "Phase 1 Scope", the initial implementation uses static peer lists for replica placement.

**Tasks**:
- [ ] Implement `push_chunk` over authenticated transport from Phase 0
- [ ] Create replica coordination: send to all target_peers in StaticReplicationHint
- [ ] Build simple confirmation tracking (success/failure per peer)
- [ ] Add offline fallback using `ReplicaFallbackPolicy`

**Implementation Location**: `crates/store/src/replicator.rs`

**Success Criteria**:
- Can replicate chunks to configured static peer list
- Failures on individual peers don't block overall replication
- Offline fallback preserves data locally until peers available
- Replica tracking persists in `/refs/<cid>` index

> **REMINDER**: Static replication is not "temporary until we have social replication". It's a permanent, supported mode for users who want explicit control. Make it clean and complete.

**Reference**: Uses unified transport from Phase 0. Chunk transfers and envelope flooding share authenticated channels.

---

### 3.4 Capability-Based Access Control

**Context**: Per [`docs/040_storage.md`](../docs/040_storage.md) Section 3 "API Surface", storage operations require specific capabilities.

**Tasks**:
- [ ] Implement `verify_storage_permissions` checking required vs granted capabilities
- [ ] Create `grant_storage_capability` issuing new capability tokens
- [ ] Build capability verification in `fetch_encrypted` operations
- [ ] Add capability checking to replica peer selection

**Implementation Location**: `crates/store/src/capability_manager.rs`

**Success Criteria**:
- Storage operations fail cleanly without required capabilities
- Capability grants are threshold-signed (if configured)
- Capability tokens carry minimal, precise permissions (no wildcards)
- Verification is constant-time to prevent timing attacks

**Code Quality Check**:
- [ ] Capability verification is pure function (no side effects)
- [ ] Grant operations are atomic (no partial grants)
- [ ] Error messages don't leak capability structure
- [ ] Zero "temporary bypass" flags or debug modes

---

## Phase 4: Integration (Week 9-10)

**Goal**: Unify SSB and Storage into a coherent system with shared infrastructure.

> **CODE QUALITY MANDATE**: Integration reveals architectural truth. If integration is painful, the architecture is wrong. Fight for clean boundaries and clear contracts.

### 4.1 Unified Peer Discovery

**Context**: Both systems need to discover peers, but with different selection criteria. From [`docs/040_storage.md`](../docs/040_storage.md) Section 5 "Unified Transport Architecture".

**Tasks**:
- [ ] Create `PeerDiscovery` trait with use-case-specific selection
- [ ] Implement storage peer selection (reliability, capacity, trust)
- [ ] Build communication peer selection (reachability, low-latency)
- [ ] Add unified peer cache combining SbbDocument.known_peers with storage replica lists

**Implementation Location**: `crates/transport/src/peer_discovery.rs`

**Success Criteria**:
- Single peer discovery API serves both use cases
- Selection criteria are explicit parameters (no hidden heuristics)
- Discovery results are deterministic for same inputs
- Peer cache updates don't race between subsystems

**Reference**: Uses SbbDocument from Phase 1.2 and StaticReplicationHint from Phase 3.1. Peer state is unified but selection is use-case-specific.

> **REMINDER**: Don't create two parallel peer discovery systems and "unify them later". Build one system with clean abstractions from the start.

---

### 4.2 Coordinated Capability Management

**Context**: From [`docs/040_storage.md`](../docs/040_storage.md) Section 2.1 "Permission" enum, capabilities span Storage, Communication, and Relay permissions.

**Tasks**:
- [ ] Implement capability grant operations covering all three permission types
- [ ] Create unified capability verification for storage access and relay permissions
- [ ] Build capability delegation chains with threshold signing
- [ ] Add capability revocation coordinating across storage and communication

**Implementation Location**: `crates/journal/src/capability/manager.rs`

**Success Criteria**:
- Single capability token can carry mixed permissions (e.g., storage + communication)
- Capability verification checks precise permission scope
- Revocation invalidates capabilities across all subsystems
- Delegation chains are verifiable without online authority

**Code Quality Check**:
- [ ] Capability manager has single responsibility (no storage or network logic)
- [ ] Grant/verify/revoke operations are pure functions
- [ ] Capability tokens are immutable value types
- [ ] No "temporary permissions" or implicit grants

---

### 4.3 Storage via Rendezvous Relationships

**Context**: Once SSB establishes a relationship, storage can use that trust for replica placement. From [`docs/040_storage.md`](../docs/040_storage.md) Section 8.1 "SBB Integration Benefits".

**Tasks**:
- [ ] Add storage capability announcement in SBB Offer envelopes
- [ ] Create storage peer selection from SbbDocument authenticated peers
- [ ] Build storage request over established relationship channels
- [ ] Implement storage confirmation in Answer envelopes

**Implementation Location**: `crates/store/src/social_storage.rs`

**Success Criteria**:
- Can discover storage capacity of communication peers
- Storage requests use existing authenticated channels (no new handshake)
- Storage relationships gracefully degrade if peer offline
- Storage availability updates don't trigger envelope floods

> **REMINDER**: This integration should feel natural, not bolted-on. If you're adding special cases, the abstraction is wrong.

**Reference**: This is the payoff for unified architecture. Communication relationships bootstrap storage trust without separate onboarding.

---

### 4.4 Coordinated Key Rotation

**Context**: Per [`docs/040_storage.md`](../docs/040_storage.md) Section 2.1 "KeyDerivationSpec", separated key derivation enables independent rotation.

**Tasks**:
- [ ] Implement relationship key rotation (K_box, K_tag, K_psk) without affecting storage keys
- [ ] Create storage key rotation without affecting relationship keys
- [ ] Build coordinated revocation: capability revocation triggers both key rotations
- [ ] Add key version tracking per subsystem
- [ ] **Future (Phase 2)**: Integrate proxy re-encryption using [rust-umbral](https://github.com/nucypher/rust-umbral) for efficient key rotation without re-encryption

**Implementation Location**: `crates/crypto/src/key_rotation.rs`

**Success Criteria**:
- Rotating relationship keys doesn't re-encrypt stored data
- Rotating storage keys doesn't invalidate envelopes
- Coordinated revocation rotates all keys atomically
- Key versions prevent replay of old operations
- **Future**: Proxy re-encryption enables gradual key migration without immediate re-encryption overhead

**Code Quality Check**:
- [ ] Rotation is atomic (no partial key updates)
- [ ] Old keys are securely zeroized after rotation
- [ ] Rotation protocol is deterministic (testable)
- [ ] No "lazy rotation" or deferred cleanup
- [ ] **Future**: Proxy re-encryption transformations are verifiable and atomic

---

## Phase 5: Production Hardening (Week 11-12)

**Goal**: Make the system production-ready with proper error handling, testing, and edge case management.

> **CODE QUALITY MANDATE**: Production code is where clean architecture proves itself. If you can't test it cleanly, you wrote it wrong. If error handling is messy, the design is messy.

### 5.1 Comprehensive Error Handling

**Context**: Production systems fail in unexpected ways. Every error should guide recovery, not obscure problems.

**Tasks**:
- [ ] Define error taxonomy: Authentication, Authorization, Network, Corruption, Resource
- [ ] Implement error context with actionable information (what failed, why, how to fix)
- [ ] Create error recovery strategies per error type
- [ ] Add structured logging with tracing for error traces

**Implementation Location**: All crates - comprehensive review

**Success Criteria**:
- Every error type has clear recovery path or user action
- Error messages don't leak sensitive information
- Error context includes request IDs for distributed tracing
- Panic-free operation under all tested error conditions

**Code Quality Check**:
- [ ] No `unwrap()` or `expect()` in production code paths
- [ ] No generic error types (e.g., `anyhow::Error` at boundaries)
- [ ] Error conversion preserves causality
- [ ] Errors are logged exactly once (no duplicate logging)

> **REMINDER**: Error handling reveals design flaws. If error propagation is complex, the call stack is wrong. Simplify the architecture, not the error handling.

---

### 5.2 Deterministic Testing Framework

**Context**: Aura's injectable effects system enables fully deterministic tests. Use it.

**Tasks**:
- [ ] Create deterministic test scenarios for all Phase 1-4 components
- [ ] Build simulation tests with controlled time, network, and randomness
- [ ] Implement Byzantine fault injection (corrupt envelopes, malicious peers)
- [ ] Add property-based tests for CRDT merge correctness

**Implementation Location**: `crates/simulator/tests/ssb_storage_tests.rs`

**Success Criteria**:
- Same seed produces identical behavior across test runs
- Can simulate days of operation in seconds
- Byzantine scenarios exercise all error paths
- Tests are fast enough for CI (< 30 seconds total)

**Reference**: Uses existing `aura-simulator` infrastructure from [`docs/006_simulation_engine_using_injected_effects.md`]. See how DKD and resharing protocols are tested.

---

### 5.3 Edge Case Management

**Context**: Production exposes edge cases theory doesn't cover. Handle them cleanly.

**Tasks**:
- [ ] Handle device addition during active envelope publishing
- [ ] Manage storage replication during relationship key rotation
- [ ] Cover capability revocation racing with ongoing operations
- [ ] Test CRDT merge conflicts under extreme conditions

**Implementation Location**: Integration tests across all crates

**Success Criteria**:
- Device operations never corrupt account state
- Key rotation doesn't lose data or create orphaned chunks
- Capability changes take effect immediately (no caching bugs)
- CRDT merges converge under all partition scenarios

**Code Quality Check**:
- [ ] Edge cases have explicit test coverage
- [ ] No "this should never happen" comments
- [ ] Race conditions have clear resolution strategy
- [ ] Timeout handling is deterministic

---

### 5.4 Performance Optimization

**Context**: Clean code first, fast code second. Optimize only with measurements.

**Tasks**:
- [ ] Profile envelope recognition performance (should be O(relationships))
- [ ] Measure CRDT merge overhead (should be < 10ms for typical documents)
- [ ] Optimize chunk encryption throughput (should saturate network)
- [ ] Benchmark capability verification (should be < 1ms)

**Implementation Location**: `crates/benches/` directory

**Success Criteria**:
- Envelope recognition handles 1000 envelopes/sec
- CRDT merges don't block envelope publishing
- Chunk operations are CPU-bound, not crypto-bound
- Capability checks are negligible overhead

> **REMINDER**: Premature optimization is the root of all evil. Measure first, optimize second, maintain clarity always.

---

## Phase 6: Advanced Features (Week 13+)

**Goal**: Build sophisticated features on the clean foundation.

> **CODE QUALITY MANDATE**: Advanced features test architectural quality. If they require hacks, the foundation is insufficient. Fix the foundation, don't hack the features.

### 6.1 Social Replica Placement

**Context**: Per [`docs/040_storage.md`](../docs/040_storage.md) Section 9 "Future Enhancement Roadmap", Phase 2 adds trust-based replication.

**Tasks**:
- [ ] Implement trust scoring based on SbbDocument peer interaction history
- [ ] Create relationship-weighted replica placement
- [ ] Build dynamic replica adjustment based on peer reliability
- [ ] Add social accountability for storage failures

**Implementation Location**: `crates/store/src/social_placement.rs`

**Success Criteria**:
- Replica placement prefers trusted, reliable peers
- Storage failures update trust scores appropriately
- Dynamic adjustment maintains target replica count
- Social accountability enables recovery coordination

**Reference**: Requires mature SBB trust metrics from Phase 2. Build on PeerAuthentication and PeerPermissions from Phase 1.2.

---

### 6.2 Proof-of-Storage

**Context**: From [`docs/040_storage.md`](../docs/040_storage.md) Section 6.1 "Corrected Proof-of-Storage Design", store chunk digests in manifest for verification.

**Tasks**:
- [ ] Add `chunk_digests` to ObjectManifest structure
- [ ] Implement challenge generation with freshness nonce
- [ ] Build verification without requiring full chunks at coordinator
- [ ] Create challenge scheduling and replica health tracking

**Implementation Location**: `crates/store/src/proof_of_storage.rs`

**Success Criteria**:
- Coordinator verifies storage without retrieving chunks
- Freshness nonce prevents replay attacks
- Challenge failures trigger replica replacement
- Challenge overhead is negligible (< 1% bandwidth)

**Code Quality Check**:
- [ ] Challenge protocol is pure cryptography (no network dependencies)
- [ ] Verification is deterministic
- [ ] Challenge scheduling doesn't DOS replicas
- [ ] Zero clever crypto (use standard constructions)

---

### 6.3 Erasure Coding

**Context**: Per [`docs/040_storage.md`](../docs/040_storage.md) Section 9, Phase 3 adds Tahoe-LAFS style erasure coding.

**Tasks**:
- [ ] Implement Reed-Solomon encoding after encryption
- [ ] Create k-of-n reconstruction with capability-based access
- [ ] Build fragment distribution across social storage network
- [ ] Add reconstruction coordination when retrieving
- [ ] **Integration**: Combine with proxy re-encryption for fragment-level capability delegation

**Implementation Location**: `crates/store/src/erasure.rs`

**Success Criteria**:
- Erasure fragments are meaningless without k shares
- Reconstruction works with any k of n peers online
- Fragment distribution respects trust boundaries
- Reconstruction is efficient (parallel fetching)
- **Future**: Proxy re-encryption enables fragment access delegation without revealing reconstruction keys

> **REMINDER**: Erasure coding is complex. Keep the API simple. Complexity belongs in the implementation, not the interface.

---

### 6.4 Economic Incentives

**Context**: From [`docs/051_rendezvous_ssb.md`](../docs/051_rendezvous_ssb.md) Post-MVP Roadmap, Phase 2 adds capability tokens and economic costs.

**Tasks**:
- [ ] Implement relay credit system (Biscuit-based capability tokens)
- [ ] Create storage quota marketplace with pricing
- [ ] Build micropayment integration for premium services
- [ ] Add reputation-based service differentiation

**Implementation Location**: `crates/journal/src/capability/economics.rs`

**Success Criteria**:
- Capability tokens can represent transferable relay credits
- Storage quotas are tradeable between users
- Micropayments are atomic with service delivery
- Reputation system prevents gaming

**Code Quality Check**:
- [ ] Economic logic separated from protocol logic
- [ ] Pricing models are pluggable (not hardcoded)
- [ ] Payment failures don't corrupt protocol state
- [ ] Zero economic logic in transport or storage layers

---

## Success Criteria: Complete System

The integrated SSB + Storage system is production-ready when:

**Functional Requirements**:
- [ ] Multi-device accounts can establish relationships and exchange messages
- [ ] Any device can store and retrieve encrypted data with capability-based access
- [ ] Relationship trust bootstraps storage replica placement
- [ ] All operations work offline with eventual consistency

**Non-Functional Requirements**:
- [ ] Zero backwards compatibility code
- [ ] Zero migration logic
- [ ] Zero legacy workarounds
- [ ] Every component has clean, focused responsibilities

**Performance Requirements**:
- [ ] Envelope recognition: 1000 envelopes/sec per device
- [ ] CRDT merges: < 10ms for typical documents
- [ ] Chunk operations: Network-bound, not CPU-bound
- [ ] Capability verification: < 1ms per check

**Testing Requirements**:
- [ ] 100% of error paths covered by tests
- [ ] All Byzantine scenarios tested deterministically
- [ ] Property-based tests verify CRDT convergence
- [ ] Integration tests cover all phase boundaries

---

## Implementation Guidelines

### Code Review Checklist

For every PR, verify:

**Architecture**:
- [ ] Clean separation between authentication and authorization
- [ ] No business logic in transport layer
- [ ] Capability checks happen at API boundaries
- [ ] CRDT operations are pure (no side effects)

**Code Quality**:
- [ ] No `unwrap()` or `expect()` in production paths
- [ ] Error types are specific, not generic
- [ ] Functions fit on one screen
- [ ] Variable names are clear and precise

**Testing**:
- [ ] Deterministic tests with controlled effects
- [ ] Byzantine fault injection exercised
- [ ] Error paths explicitly tested
- [ ] Performance benchmarks included

**Documentation**:
- [ ] API documentation explains "why", not just "what"
- [ ] Error conditions documented
- [ ] Invariants stated explicitly
- [ ] Examples are realistic, not toy code

### Common Pitfalls to Avoid

**Don't**:
- Add "temporary" feature flags that become permanent
- Write "TODO: clean this up" comments (clean it up now)
- Mix authentication and authorization logic
- Put business logic in protocol choreographies
- Hand-roll cryptography
- Optimize before measuring
- Accumulate optional fields in core types
- Create parallel systems that "will be unified later"

**Do**:
- Write the simplest code that could possibly work
- Separate concerns cleanly
- Make illegal states unrepresentable
- Use types to enforce invariants
- Test deterministically with injectable effects
- Document invariants and assumptions
- Refactor continuously as understanding improves

---

## Dependencies and Prerequisites

### Existing Infrastructure (Already Implemented)

From Aura Phase 0-3:
- [x] Threshold signatures (FROST Ed25519)
- [x] CRDT-based account ledger (Automerge)
- [x] Injectable effects system for deterministic testing
- [x] Device certificate management
- [x] Basic transport abstraction
- [x] Choreographic protocol infrastructure

### New Infrastructure (This Plan)

Phase 0:
- [ ] Unified capability system (Storage, Communication, Relay)
- [ ] Separated key derivation (Identity vs Permission contexts)
- [ ] Unified transport interface with connection pooling

Phase 1:
- [ ] Multi-device relationship key coordination
- [ ] SBB CRDT document structure
- [ ] Envelope format and CID computation
- [ ] Threshold-signed counter coordination

Phase 2:
- [ ] Envelope publishing and recognition
- [ ] CRDT-based gossip with rate limiting
- [ ] Offer/Answer rendezvous choreography

Phase 3:
- [ ] Object manifest structure
- [ ] Chunk encryption and local storage
- [ ] Static replica coordination
- [ ] Capability-based storage access control

Phase 4:
- [ ] Unified peer discovery
- [ ] Coordinated capability management
- [ ] Storage via rendezvous relationships
- [ ] Coordinated key rotation

### External Dependencies

**Rust Crates** (already in workspace):
- `frost-ed25519`: Threshold signatures
- `automerge`: CRDT infrastructure
- `quinn`: QUIC transport
- `snow`: Noise protocol
- `chacha20poly1305`: AEAD encryption
- `blake3`: Hashing
- `redb`: Local storage
- `tokio`: Async runtime

**Future Dependencies (Phase 2+)**:
- `umbral-pre`: Threshold proxy re-encryption ([rust-umbral](https://github.com/nucypher/rust-umbral))

**No New External Dependencies Required for Phase 1**

---

## Timeline and Resource Allocation

**Total Duration**: 12 weeks (minimum)

**Phase 0** (Foundation): 2 weeks, 1 developer
**Phase 1** (SSB Core): 2 weeks, 1 developer  
**Phase 2** (SSB Operations): 2 weeks, 1 developer
**Phase 3** (Storage Core): 2 weeks, 1 developer
**Phase 4** (Integration): 2 weeks, 1 developer
**Phase 5** (Hardening): 2 weeks, 1 developer
**Phase 6** (Advanced): Ongoing, as needed

**Critical Path**: Phases 0-5 are sequential. Phase 6 is parallel/optional.

**Risk Factors**:
- CRDT merge complexity may require additional optimization
- Multi-device counter coordination edge cases may need iteration
- Integration may reveal architectural issues requiring refactoring

**Mitigation**: Build incrementally, test deterministically, refactor continuously. The injectable effects system makes iteration fast.

---

## Conclusion

This plan delivers a uniquely integrated system where communication relationships and storage trust are unified. The clean separation of authentication and authorization, combined with capability-based access control, creates a foundation that scales from MVP to advanced features without architectural rework.

**Remember**: Concise, clean, elegant code. Zero backwards compatibility. Zero migration logic. Zero legacy code. Every line serves a clear purpose. Every abstraction earns its keep. Every component has a single responsibility.

Build it right the first time.
