# 120 · Keyhive Integration for Authorization and Group Key Agreement

**Status:** Design Proposal
**Version:** 2.0
**Created:** October 23, 2025
**Target:** Phase 3

## 1. Executive Summary

This document proposes the comprehensive integration of the `keyhive` library as both the authorization system and Continuous Group Key Agreement (CGKA) protocol for Aura. This approach would replace both the previously considered CRDT-native OpenMLS integration (outlined in `110_crdt_native_mls_integration.md`) and Aura's current biscuit-based policy engine with Keyhive's unified architecture.

Keyhive is a local-first authorization system that combines convergent capabilities with the `BeeKEM` protocol, creating a coherent foundation for both authorization and encryption in CRDT environments. This integration represents a fundamental architectural shift that aligns Aura's authorization model with local-first principles.

The key trade-offs are the lack of forward secrecy in the causal encryption layer (required for CRDT historical access) and the adoption of unproven convergent capabilities technology. However, these trade-offs enable a unified architecture where authorization and encryption access are managed by a single, eventually consistent authority graph.

## 2. Motivation

Aura's current architecture has two fundamental misalignments that this proposal addresses:

### 2.1. Authorization System Mismatch

Aura's biscuit-based policy engine assumes linearizable policy evaluation, which conflicts with the eventually consistent nature of CRDT-based systems. This creates several problems:

*   **Impedance Mismatch:** Biscuits require authoritative policy servers, while CRDTs operate in partition-tolerant environments
*   **Complex Synchronization:** Separate authorization and encryption systems require complex coordination
*   **Revocation Complexity:** Biscuit revocation in eventually consistent environments is non-trivial

### 2.2. CGKA Protocol Limitations

The previously considered OpenMLS integration faces significant technical challenges:

*   **Limited Public API:** OpenMLS doesn't expose sufficient interfaces for CRDT integration
*   **Linearizability Requirements:** TreeKEM requires strict ordering, incompatible with CRDT semantics
*   **Engineering Overhead:** "Compose, Don't Fork" approach requires reimplementing most of MLS state management

### 2.3. Keyhive's Unified Solution

Keyhive offers a compelling architectural alternative that addresses both challenges:

*   **Convergent Capabilities:** A CRDT-native authorization model designed for eventual consistency
*   **BeeKEM Protocol:** A concurrent TreeKEM variant that works with CRDT conflict resolution
*   **Unified Authority Graph:** Single system managing both authorization and encryption access
*   **Local-First Design:** Built from the ground up for partition-tolerant, eventually consistent systems
*   **Identity Agnostic:** Clean integration with Aura's threshold identity system

This represents a fundamental architectural improvement: instead of forcing server-centric authorization models into local-first environments, we adopt a system designed specifically for this use case.

## 3. Proposed Architecture

This integration fundamentally restructures Aura's authorization and encryption architecture around Keyhive's unified model. The system will be built on convergent capabilities that drive both policy evaluation and group key agreement.

### 3.1. Unified Architecture

Keyhive replaces both Aura's policy engine and provides CGKA functionality:

```
┌─────────────────────────────────────────────────┐
│          Application Layer                       │
│  (Private DAOs, Group Storage, Messaging)       │
└────────────────────┬────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────┐
│          Identity Layer (Aura)                   │
│                                                  │
│  • Threshold identity management                │
│  • DKD for context-specific keys                │
└────────────────────┬────────────────────────────┘
                     │ Provides Keys
┌────────────────────▼────────────────────────────┐
│      Authorization & CGKA Layer (Keyhive)       │
│                                                  │
│  • Convergent capabilities for authorization    │
│  • `BeeKEM` for concurrent group key agreement  │
│  • Unified authority graph                      │
│  • Visibility index for revocation handling     │
└────────────────────┬────────────────────────────┘
                     │ Generates
┌────────────────────▼────────────────────────────┐
│       Encryption Layer                           │
│                                                  │
│  • Causal encryption with predecessor keys      │
│  • `ApplicationSecret` for E2E encryption       │
└────────────────────┬────────────────────────────┘
                     │ Uses
┌────────────────────▼────────────────────────────┐
│          Transport Layer (Aura)                  │
│  (P2P delivery of operations and capabilities)  │
└─────────────────────────────────────────────────┘
```

### 3.2. Integration Details

#### 3.2.1. Authorization System Replacement

Keyhive's convergent capabilities will completely replace Aura's biscuit-based policy engine:

*   **Capability Documents:** Replace policy documents with capability delegation chains stored in the CRDT ledger
*   **Authority Graph:** Maintain a stateful view of who can authorize what operations
*   **Visibility Index:** Operations remain available for causality but materialize based on current authorization state
*   **Revocation Cascades:** Handle complex revocation scenarios where revokers themselves get revoked

#### 3.2.2. Identity Integration

Aura retains control over identity while Keyhive manages authorization, with full harmonization across all identity systems:

*   **Threshold Signatures:** Aura's threshold identity system signs capability delegations and revocations
*   **DKD Integration:** Use deterministic key derivation to generate context-specific keys for capability operations
*   **IndividualId Mapping:** Keyhive operations reference Aura's `IndividualId`s for consistency
*   **Device Certificate Harmonization:** Integrate Aura's device certificates with Keyhive's identity model, ensuring device credentials are consistently recognized across threshold identity, capability evaluation, and transport layers
*   **Unified Identity Proofs:** Device certificates signed by threshold identity serve as authoritative identity proofs for both capability operations and peer-to-peer connections

#### 3.2.3. CRDT Integration

Capability operations, CGKA operations, and SBB coordination all become part of the unified CRDT history:

```rust
// in crates/journal/src/events.rs

pub enum Event {
    // ... existing events
    CapabilityDelegation(keyhive_core::capability::Delegation),
    CapabilityRevocation(keyhive_core::capability::Revocation),
    KeyhiveCgka(keyhive_core::cgka::operation::CgkaOperation),
    
    // SBB integration events
    SbbEnvelopePublished { relationship_id: RelationshipId, epoch: u32, envelope: SealedEnvelope },
    SbbRelayPermissionGranted { peer_id: PeerId, capability_id: CapabilityId },
    SbbNeighborAdded { peer_id: PeerId, via_capability: CapabilityId },
    SbbQuotaAssigned { peer_id: PeerId, quota: EnvelopeQuota },
}
```

The unified authority graph drives authorization decisions, CGKA membership, and SBB relay permissions, eliminating synchronization complexity across all coordination layers.

#### 3.2.4. Capability Event Model

Capability mutations are encoded as CRDT events with explicit merge semantics:

*   **Delegation Payload:** `{capability_id, parent_id, subject_id, scope, expiry, proof}` where `capability_id` is a deterministic hash of the parent chain, ensuring idempotent replays.
*   **Revocation Payload:** `{capability_id, revoked_at, reason, proof}` referencing the same identifiers; revocations win over delegations via a last-writer-wins clock ordered by `(revoked_at, author_device_id)`.
*   **Validation Invariants:** (1) every delegation must reference an existing, unrevoked parent; (2) revocations must be signed by an authority in the ancestor chain; (3) timestamp monotonicity per `capability_id`.
*   **Conflict Resolution:** Concurrent delegations with the same parent are both admitted; concurrent revocation/delegation conflicts resolve deterministically because revocations redact the capability from the visibility index while leaving the historical event intact.

These rules are enforced within the CRDT apply path so replicas independently reach the same capability graph and revocation state.

#### 3.2.5. Capability → BeeKEM Membership Pipeline

Capability evaluations deterministically drive BeeKEM membership:

1.  **Eligibility View:** Each replica computes the current capability graph and extracts subjects granted the `mls/member` scope for a given group.
2.  **Ordering Rule:** Subjects are sorted by `(capability_id, subject_id)`—both stable identifiers—to produce the canonical roster ordering that feeds BeeKEM.
3.  **Delta Projection:** Comparing the previous roster against the new eligibility view yields a deterministic list of `Join`, `Update`, and `Remove` operations that are emitted as `KeyhiveCgka` events tagged with the target epoch.
4.  **Failure Handling:** If BeeKEM rejects an operation (e.g., due to stale epoch), the event is marked failed and re-queued after fetching the latest roster snapshot, preventing divergent retries.

Because the pipeline is fully derived from CRDT state, replicas remain in lockstep without an external sequencer.

#### 3.2.6. SBB Integration

The Social Bulletin Board (SBB) system harmonizes with Keyhive's unified authorization model:

**Capability-Driven Envelope Publishing:**
*   **Publishing Rights:** Replace simple counter coordination with capability-based envelope publishing rights
*   **Quota Management:** Use convergent capabilities to grant and revoke SBB relay quotas dynamically
*   **Relay Permissions:** Authority graph controls who can relay envelopes through the network

**Unified Spam Prevention:**
*   **Layered Defense:** Combine Keyhive capability-based quotas with complementary social rate limiting
*   **Social + Cryptographic:** Capability tokens provide cryptographic publishing rights while social rate limiting adds behavioral accountability
*   **Dynamic Quotas:** Capability system can adjust envelope quotas based on trust relationships and behavior

**Transport Layer Harmonization:**
*   **Unified Keys:** Replace separate pairwise secrets with Keyhive-derived keys for SBB envelope encryption
*   **Identity Proofs:** Device certificates serve as unified identity proofs for both capability operations and SBB handshakes
*   **Channel Establishment:** Integrate SBB rendezvous with Keyhive's group key agreement for seamless transitions

**Event Schema Integration:**
*   **SbbDocument CRDT:** Merge SBB's separate CRDT with the main authority graph CRDT for unified state management
*   **Visibility Control:** Use Keyhive's visibility index to control SBB envelope materialization based on current authorization
*   **Revocation Cascades:** Automatic capability revocation removes SBB relay permissions and invalidates pending envelopes

**Neighbor Trust Management:**
*   **Authority-Based Neighbors:** Replace manual web-of-trust with authority graph delegation chains
*   **Automatic Pruning:** Capability revocation automatically removes untrusted neighbors
*   **Social Circuit Breaker:** Preserve social rate limiting as behavioral backstop for compromised-but-authorized nodes

#### 3.2.7. Cutover Strategy

The transition replaces biscuits outright while preserving a reversible recovery point:

1.  **Policy Translation:** Convert existing biscuit policies to capability delegations offline and verify equivalence via deterministic replay tests.
2.  **Capability Bootstrap:** Publish a signed `CutoverIntent` ledger entry that seeds the initial authority graph and records the biscuit policy snapshot hash.
3.  **Cutover Epoch:** Upon quorum acknowledgement, clients disable biscuit evaluation and start enforcing capabilities only; the ledger records the cutover epoch so late clients can refuse pre-cutover operations.
4.  **Recovery Plan:** If a critical issue is found, a `CutoverRollback` entry re-enables biscuits using the preserved snapshot hash, ensuring we can revert without running both systems simultaneously.

## 4. Security Considerations

### 4.1. Layered Security Model

Keyhive's architecture provides different security properties at different layers:

#### 4.1.1. BeeKEM Protocol Security

The BeeKEM CGKA protocol maintains standard TreeKEM security properties:

*   **Forward Secrecy:** Key ratcheting ensures past keys cannot be recovered from current keys
*   **Post-Compromise Security:** Member removal prevents future key derivation by compromised parties
*   **Concurrent Safety:** Handles concurrent operations while maintaining security properties

#### 4.1.2. Causal Encryption Trade-offs

The application-layer causal encryption deliberately sacrifices forward secrecy for CRDT functionality:

*   **Historical Access:** Keys to causal predecessors are included to enable CRDT history access
*   **Trade-off Rationale:** For collaborative documents, accessing historical states is often a feature
*   **Scoped Impact:** Forward secrecy loss is limited to the application layer, not the underlying CGKA

**Implications:**

*   CGKA layer maintains forward secrecy through ratcheting
*   Application layer trades forward secrecy for causal document access
*   Compromised keys can decrypt historical application data the member accessed
*   Future keys remain secure through post-compromise security

**Mitigation:**

1.  **Layered Security:** CGKA and application encryption provide defense in depth
2.  **Automatic Rotation:** Convergent capabilities can enforce periodic key rotation
3.  **Access Control:** Visibility index restricts materialization based on current authorization
4.  **Threat Model Alignment:** Historical access often acceptable for collaborative applications

### 4.2. Unproven Capability System

Convergent capabilities represent a novel approach to authorization in CRDT environments. Unlike biscuits, which have extensive formal verification and industry adoption, convergent capabilities are experimental.

**Implications:**

*   **Novel Attack Vectors:** Untested revocation cascade logic could have subtle vulnerabilities
*   **Complexity Risk:** The stateful authority graph and visibility index introduce new failure modes
*   **Research Phase:** Limited real-world validation of the capability model

**Mitigation:**

1.  **Formal Verification:** Commission formal verification of the convergent capability semantics
2.  **Extensive Testing:** Implement comprehensive test suites covering revocation cascades and conflict resolution
3.  **Security Audit:** Third-party audit of both BeeKEM and capability system before production use
4.  **Gradual Rollout:** Deploy in low-risk scenarios first to validate security properties

### 4.3. Custom Cryptographic Protocol

BeeKEM is a custom concurrent variant of TreeKEM. While inspired by well-understood protocols, any custom cryptographic implementation carries inherent risks.

**Mitigation:**

1.  **Security Audit:** Comprehensive third-party cryptographic audit of BeeKEM implementation
2.  **Formal Verification:** Long-term goal to formally verify BeeKEM's security properties
3.  **Conservative Deployment:** Extensive testing in controlled environments before broader adoption

## 5. Implementation Plan

The integration represents a fundamental architectural shift requiring careful phased implementation:

### Phase 1: Capability System Foundation (6 weeks)

*   **Dependency Management:** Add `keyhive_core` as a dependency to the Aura workspace.
*   **Capability Events:** Implement the delegation/revocation event schema, merge rules, and invariant checks in the CRDT apply path.
*   **Authority Graph:** Build convergent capability state management backed by deterministic snapshots.
*   **Visibility Index:** Materialize operation visibility from the capability graph and enforce revocation cascades.
*   **Deterministic Replay Tests:** Create fixtures that replay historical biscuit policies as capabilities to prove parity.

### Phase 2: Policy Engine Replacement (4 weeks)

*   **Policy Translation:** Convert existing biscuit policies to capability delegations and validate via offline differencing.
*   **Threshold Integration:** Use Aura's threshold signatures for capability bootstrapping and revocation proofs.
*   **Cutover Ledger Entries:** Implement `CutoverIntent` and `CutoverRollback` events and client handling logic.
*   **Operator Runbooks:** Document pre-cutover validation, quorum acknowledgement, and rollback procedures.

### Phase 3: CGKA Integration (3 weeks)

*   **Unified Authority:** Wire the capability graph into the BeeKEM roster builder using the deterministic pipeline.
*   **BeeKEM Operations:** Emit roster deltas as `KeyhiveCgka` events and harden epoch conflict handling.
*   **Key Agreement:** Derive group secrets from the BeeKEM roster and publish sealed artifacts for clients.
*   **Causal Encryption:** Extend the encryption layer to consume the new `ApplicationSecret` derivations.

### Phase 4: SBB Integration & Harmonization (3 weeks)

*   **SBB Event Schema:** Implement SBB-specific events in the unified CRDT ledger and capability-driven envelope publishing
*   **Transport Harmonization:** Update SBB to use Keyhive-derived keys and unified device certificates for identity proofs
*   **Spam Prevention:** Implement layered defense combining capability-based quotas with complementary social rate limiting
*   **Neighbor Management:** Replace manual web-of-trust with authority graph delegation chains while preserving social circuit breakers

### Phase 5: Application Integration & Data Migration (4 weeks)

*   **End-to-End Encryption:** Update clients and demos to consume capability-driven secrets across all layers (storage, messaging, SBB)
*   **Historical Re-Encryption:** Build tooling to re-encrypt stored artifacts with capability-derived keys and tag content with the cutover epoch
*   **Client Upgrade Path:** Ship staged client releases that can read both pre-cutover and post-cutover payloads
*   **Testing Framework:** Expand integration tests to cover data migration, roster churn, revocation races, and SBB harmonization

### Phase 6: Production Hardening (Ongoing)

*   **Security Audit:** Commission third-party audits of both the capability system and BeeKEM implementation.
*   **Formal Verification:** Begin formal verification of convergent capability semantics.
*   **Monitoring:** Implement operational telemetry for capability evaluations, roster churn, and migration health.
*   **Post-Cutover Review:** After cutover, run a blameless review using replay fixtures to confirm parity with biscuit-era policies.

### Data Migration Guarantees

The cutover tooling performs staged re-encryption to avoid data loss:

1.  **Snapshot Freeze:** Prior to `CutoverIntent`, freeze write access and capture a content snapshot hash.
2.  **Batch Re-Encryption:** Re-encrypt stored artifacts in deterministic batches; each batch records `{content_id, old_key_hash, new_key_hash}` in an audit log synced through the CRDT.
3.  **Verification:** Clients verify migrated payloads by decrypting with both key versions and comparing hashes before acknowledging the batch.
4.  **Rollback Safety:** If a batch fails verification, the audit log allows regenerating the pre-cutover ciphertext using the preserved biscuit key material until the issue is resolved.

## 6. Conclusion

This proposal represents a fundamental architectural evolution for Aura, adopting Keyhive's unified approach to authorization and encryption across all system layers—including the Social Bulletin Board (SBB) for peer discovery and transport coordination. By replacing the biscuit-based policy engine, implementing BeeKEM for group key agreement, and harmonizing SBB operations, we achieve several critical advantages:

**Comprehensive Architectural Coherence:** A single, eventually consistent authority graph manages authorization decisions, encryption access, and peer-to-peer coordination, eliminating complex synchronization between separate systems across all layers.

**Unified Identity Model:** Device certificates, threshold identity, and Keyhive identity are fully harmonized, providing consistent identity proofs from capability evaluation through transport establishment.

**Local-First Alignment:** Convergent capabilities are designed specifically for partition-tolerant, eventually consistent environments, providing natural alignment with Aura's CRDT-based architecture across storage, messaging, and peer discovery.

**Layered Security Defense:** The integration preserves complementary social rate limiting alongside cryptographic capability-based quotas, providing robust spam prevention that combines behavioral accountability with cryptographic access control.

**Unified Security Model:** The capability system's visibility index elegantly handles revocation cascades while maintaining causal consistency, solving complex authorization problems across all coordination layers that are difficult to address with traditional policy engines.

**Future-Proof Foundation:** This architecture provides a robust foundation for complex collaborative applications that require sophisticated delegation, revocation, encryption, and peer discovery capabilities.

The key trade-offs—application-layer forward secrecy for CRDT historical access and adoption of unproven convergent capability technology—are balanced by the comprehensive architectural benefits and strong CGKA security properties. The phased implementation plan ensures careful validation of security properties while managing the complexity of this architectural transition across all system layers.

This proposal recommends proceeding with comprehensive Keyhive integration as it represents the most coherent path toward a truly local-first authorization, encryption, and coordination system, with the explicit requirement for comprehensive security auditing before production deployment.
