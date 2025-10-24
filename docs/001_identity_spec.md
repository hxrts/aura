# 030 · Identity Specification (Phase 0)

## 0. Identity Philosophy: Relational Security

Aura’s identity system is built on a philosophy of **relational security**, formalizing the trust that already exists in real-world social connections rather than relying on centralized authorities or abstract proofs of personhood. An Aura account is conceptualized as a **private, single-member Decentralized Autonomous Organization (DAO)**, where the user is the primary member, and trusted contacts (guardians) are invited to fulfill specific, limited roles.

Key principles guiding this approach:
-   **Trust Through Action:** Adding a device or guardian is a deliberate, meaningful action that grounds trust in established relationships.
-   **Privacy by Default:** The social graph and relationships between accounts remain confidential, never published to a global, transparent system.
-   **Generalization:** The underlying primitives (accounts, authorizations, policies) are flexible enough to model various multi-party arrangements, from social recovery to shared resources.

This approach ensures that identity management is deeply integrated with the user's actual trust network, providing a robust and human-centric security model.

## 1. Key Material

- **Prototype root key**
  - `pk_root_ed25519` – threshold FROST key used for every signing and derivation path in the prototype.
  - Future iterations will introduce a passkey/WebAuthn-compatible root; shares are scoped to this prototype only.
- **Device sealing**
  - Devices derive a local symmetric key via OS keystore or software KDF (no hardware requirement for the prototype).
  - Threshold share is stored encrypted-at-rest; decrypted only during MPC windows.

## 2. Deterministic Key Derivation (DKD)

Purpose: derive app/context-specific keys without revealing linkage.

### 2.1 Context Capsule

```rust
struct ContextCapsule {
    app_id: String,
    context_label: String,
    policy_hint: Option<Cid>,
    transport_hint: Option<String>,
    ttl: Option<u64>,         // seconds (default 24h)
    issued_at: u64,           // unix seconds
}
```

Only `app_id`, `context_label`, `ttl`, and `issued_at` are mandatory. `policy_hint`
and `transport_hint` remain optional for advanced apps.

### 2.2 DKD Steps

0. Canonicalise the capsule to deterministic CBOR (sorted keys, omitted `None`
   fields) and compute  
   `context_id = BLAKE3("aura.context_id.v1" || cbor_capsule)`.
1. Each participant computes `H_i = BLAKE3("aura.dkd.v1" || share_i || context_id)`.
2. Sum the points: `P = Σ H_i·G`.
3. Derive `seed_capsule` from `P` using the curve-specific mapping (see notes below). Immediately zeroise after use.
4. Expand seed with HKDF to target key type (Ed25519/X25519/etc.).
5. Produce `capsule_mac = BLAKE3_keyed(seed_capsule, cbor(capsule))` for tamper detection.
6. Optionally run threshold signing to produce a binding proof tying `pk_derived` to `pk_root`.

**Curve-specific mapping**
- **Ed25519:** convert `P` to the Edwards form, clear the cofactor (`P' = [8]P`), and use the 32-byte little-endian encoding of `P'`'s x-coordinate reduced modulo `ℓ` (the Ed25519 subgroup order) as `seed_capsule`. Reject the rare case where `P'` is identity and rerun DKD with a fresh nonce.

### 2.3 Developer Helpers

```rust
impl DeviceAgent {
    pub async fn derive_simple_identity(
        &self,
        app_id: &str,
        context_label: &str,
    ) -> Result<(DerivedIdentity, PresenceTicket)> {
        let capsule = ContextCapsule {
            app_id: app_id.to_string(),
            context_label: context_label.to_string(),
            policy_hint: None,
            transport_hint: None,
            ttl: Some(24 * 3600),
            issued_at: now(),
        };
        let identity = self.derive_context_identity(&capsule, false).await?;
        let ticket = self.issue_presence_ticket(&identity).await?;
        Ok((identity, ticket))
    }
}
```

Apps call the helper for the common case; advanced scenarios build the capsule manually.

## 3. Presence Tickets & Session Epoch

- `session_epoch` lives in the CRDT as a monotonic counter.
- `PresenceTicket` structure:

```rust
struct PresenceTicket {
    issued_by: DeviceId,
    expires_at: u64,
    capability: Vec<u8>, // e.g., Biscuit or HPKE-wrapped secret
}
```

- On suspected compromise or share refresh, quorum bumps the epoch (`bump_session_epoch()`), clearing cached tickets.
- Handshake secret = HKDF(seed_capsule || session_epoch || handshake_nonce).
- Transports reject mismatched epochs with constant-time responses to prevent presence probing.

## 4. Authenticated CRDT Ledger

### 4.1 Stored Fields

- Devices & guardians (G-Set with metadata).
- Policy entries (threshold-signed manifest references).
- Session epoch + presence ticket cache.
- Cooldown counters, DKD backoff state.

### 4.2 Event Types

| Event                 | Authorization            | CRDT Representation                         |
|-----------------------|--------------------------|---------------------------------------------|
| AddDevice             | Threshold signature      | Signed event stored as blob, CRDT adds ID   |
| RemoveDevice          | Threshold signature      | Signed event + CRDT tombstone               |
| AddGuardian           | Threshold signature      | Signed event + guardian entry               |
| PolicyUpdate          | Threshold signature      | Manifest reference + version bump           |
| SessionEpochBump      | Threshold signature      | Increments `session_epoch`, clears tickets  |
| CooldownTick          | Device certificate sig   | PN-Counter entry                            |
| PresenceTicketCache   | Device certificate sig   | Map of `peer -> ticket metadata`            |

High-impact events must carry a threshold signature and are stored as immutable
blobs (CID referenced in the CRDT). High-churn, low-impact updates remain device-signed.

### 4.3 Verification

- On ingest, replicas validate:
  - Threshold signatures for event blobs.
  - Device certificates for device-signed operations.
  - Monotonic version checks (e.g., policy version, epoch counter).

## 5. Device & Guardian Lifecycle

1. **Enrollment** – invite signed by quorum; new device receives encrypted shares + device certificate.
2. **Rotation** – proactive refresh runs periodically; shares and device certificate updates recorded in CRDT.
3. **Revocation** – threshold event removes device, bumps session epoch, clears tickets.
4. **Guardian onboarding** – threshold event + share distribution; stored in CRDT with contact metadata.
5. **Guardian replacement** – same process; old guardian entry tombstoned after cooldown (policy-defined).

### 5.1 Guardian Share Handling

1. **Generation** – During guardian onboarding, the orchestrator derives a recovery-only share by evaluating a fresh polynomial seeded from `pk_root_ed25519`’s secret scalar (`recovery_share = HKDF(secret_root || guardian_id || "aura.recovery.v1")`). The polynomial coefficients are threshold-signed and referenced in the CRDT event.
2. **Packaging** – Recovery shares are wrapped in a `RecoveryShareEnvelope` containing the share bytes, guardian contact fingerprint, and expiry metadata. The envelope is HPKE-encrypted to the guardian’s device certificate and stored as a manifest referenced by the onboarding event.
3. **Rotation** – Whenever the root shares are reshared (device rotation or recovery completion), a new polynomial is sampled; fresh envelopes replace the previous manifests, and the CRDT records the new envelope CID with a monotonic version.
4. **Revocation** – Removing a guardian tombstones the envelope reference and queues the old manifest CID for cryptographic erasure. Session epoch bumps invalidate any cached presence tickets so a removed guardian cannot respond to MPC invitations.

## 6. Recovery Flow (Identity Perspective)

1. Legitimate user initiates recovery (device lost).  
2. Guardians approve via threshold-signed events.  
3. Cooldown enforced by CRDT counter; user can cancel during window.  
4. After cooldown, resharing MPC issues new device share; session epoch bumped; presence tickets reissued.

## 7. Open Questions / Future Work

- Hardware-backed MPC: track progress of native APIs to move DKD/threshold operations inside Secure Elements.
- Service signer roles: integrate semi-trusted co-signers once Cedar policies mature.
- Cross-account delegation: future extension for shared resources (out of scope for Phase 0/1).***
