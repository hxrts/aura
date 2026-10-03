# Cryptography

This document describes the cryptographic architecture in Aura. It defines layer responsibilities, code organization patterns, security invariants, and compliance requirements for cryptographic operations.

## 1. Overview

Aura's cryptographic architecture follows the 8-layer system design with strict separation of concerns.

- Layer 1 (`aura-core`): Type wrappers, trait definitions, pure functions
- Layer 3 (`aura-effects`): Production implementations with real crypto libraries
- Layer 8 (`aura-testkit`): Mock implementations for deterministic testing

This separation ensures that cryptographic operations are auditable, testable, and maintainable. Security review focuses on a small number of files rather than scattered usage throughout the codebase.

## 2. Layer Responsibilities

### 2.1 Layer 1: aura-core

The `aura-core` crate provides cryptographic foundations without direct side effects.

#### Type Wrappers

Type wrappers live in `crates/aura-core/src/crypto/ed25519.rs`.

```rust
pub struct Ed25519SigningKey(pub [u8; 32]);
pub struct Ed25519VerifyingKey(pub [u8; 32]);
pub struct Ed25519Signature(pub [u8; 64]);
```

These wrappers use fixed-size arrays for type safety and delegate to `ed25519_dalek` internally. They expose a stable API independent of the underlying library. They enable future algorithm migration without changing application code. They provide type safety across crate boundaries.

#### Effect Traits

Effect trait definitions live in `crates/aura-core/src/effects/`. The `CryptoCoreEffects` trait inherits from `RandomCoreEffects` and provides core cryptographic operations.

```rust
#[async_trait]
pub trait CryptoCoreEffects: RandomCoreEffects + Send + Sync {
    // Key derivation
    async fn kdf_derive(&self, ikm: &[u8], salt: &[u8], info: &[u8], output_len: u32) -> Result<Vec<u8>, CryptoError>;
    async fn derive_key(&self, master_key: &[u8], context: &KeyDerivationContext) -> Result<Vec<u8>, CryptoError>;

    // Ed25519 signatures
    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError>;
    async fn ed25519_sign(&self, message: &[u8], private_key: &[u8]) -> Result<Vec<u8>, CryptoError>;
    async fn ed25519_verify(&self, message: &[u8], signature: &[u8], public_key: &[u8]) -> Result<bool, CryptoError>;

    // Utility methods
    fn is_simulated(&self) -> bool;
    fn crypto_capabilities(&self) -> Vec<String>;
    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool;
    fn secure_zero(&self, data: &mut [u8]);
}
```

The `CryptoExtendedEffects` trait provides additional operations with default implementations that return errors:

```rust
#[async_trait]
pub trait CryptoExtendedEffects: CryptoCoreEffects + Send + Sync {
    // Unified signing API
    async fn generate_signing_keys(&self, threshold: u16, max_signers: u16) -> Result<SigningKeyGenResult, CryptoError>;
    async fn generate_signing_keys_with(&self, method: KeyGenerationMethod, threshold: u16, max_signers: u16) -> Result<SigningKeyGenResult, CryptoError>;
    async fn sign_with_key(&self, message: &[u8], key_package: &[u8], mode: SigningMode) -> Result<Vec<u8>, CryptoError>;
    async fn verify_signature(&self, message: &[u8], signature: &[u8], public_key_package: &[u8], mode: SigningMode) -> Result<bool, CryptoError>;

    // FROST threshold signatures
    async fn frost_generate_keys(&self, threshold: u16, max_signers: u16) -> Result<FrostKeyGenResult, CryptoError>;
    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError>;
    async fn frost_create_signing_package(&self, message: &[u8], nonces: &[Vec<u8>], participants: &[u16], public_key_package: &[u8]) -> Result<FrostSigningPackage, CryptoError>;
    async fn frost_sign_share(&self, signing_package: &FrostSigningPackage, key_share: &[u8], nonces: &[u8]) -> Result<Vec<u8>, CryptoError>;
    async fn frost_aggregate_signatures(&self, signing_package: &FrostSigningPackage, signature_shares: &[Vec<u8>]) -> Result<Vec<u8>, CryptoError>;
    async fn frost_verify(&self, message: &[u8], signature: &[u8], group_public_key: &[u8]) -> Result<bool, CryptoError>;
    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError>;

    // Symmetric encryption
    async fn chacha20_encrypt(&self, plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    async fn chacha20_decrypt(&self, ciphertext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    async fn aes_gcm_encrypt(&self, plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    async fn aes_gcm_decrypt(&self, ciphertext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;

    // Key rotation and conversion
    async fn frost_rotate_keys(&self, old_shares: &[Vec<u8>], old_threshold: u16, new_threshold: u16, new_max_signers: u16) -> Result<FrostKeyGenResult, CryptoError>;
    async fn convert_ed25519_to_x25519_public(&self, ed25519_public_key: &[u8]) -> Result<[u8; 32], CryptoError>;
    async fn convert_ed25519_to_x25519_private(&self, ed25519_private_key: &[u8]) -> Result<[u8; 32], CryptoError>;
}

pub trait CryptoEffects: CryptoCoreEffects + CryptoExtendedEffects {}
```

The core trait provides key derivation and Ed25519 signatures. The extended trait provides unified signing that routes between single-signer and threshold modes, FROST threshold operations, symmetric encryption, and key conversion. Hashing is not included because it is a pure operation. Use `aura_core::hash::hash()` for synchronous hashing instead.

The `RandomCoreEffects` trait provides cryptographically secure random number generation.

```rust
#[async_trait]
pub trait RandomCoreEffects: Send + Sync {
    async fn random_bytes(&self, len: usize) -> Vec<u8>;
    async fn random_bytes_32(&self) -> [u8; 32];
    async fn random_u64(&self) -> u64;
}

#[async_trait]
pub trait RandomExtendedEffects: RandomCoreEffects + Send + Sync {
    async fn random_range(&self, min: u64, max: u64) -> u64;
    async fn random_uuid(&self) -> Uuid;
}
```

The core trait provides basic random generation. The extended trait adds range and UUID generation with default implementations. All randomness flows through these traits for testability and simulation.

Pure functions in `crates/aura-core/src/crypto/` implement hash functions, signature verification, and other deterministic operations. These require no side effects and can be called directly.

### 2.2 Layer 3: aura-effects

The `aura-effects` crate contains the only production implementations that directly use cryptographic libraries.

The production handler lives in `crates/aura-effects/src/crypto.rs`. `RealCryptoHandler` can operate with OS entropy (production) or with a seed (deterministic testing). It implements all methods from `CryptoCoreEffects` and `RandomCoreEffects`.

The following direct imports are allowed in Layer 3:

- `ed25519_dalek`
- `frost_ed25519`
- `chacha20poly1305`
- `aes_gcm`
- `getrandom`
- `rand_core::OsRng`
- `rand_chacha`
- `blake3`

### 2.3 Threshold Lifecycle (K1/K2/K3) and Transcript Binding

Aura separates key generation from agreement/finality:

- **K1**: Single-signer (Ed25519). No DKG required.
- **K2**: Dealer-based DKG. A trusted coordinator produces dealer packages.
- **K3**: Consensus-finalized DKG. The BFT-DKG transcript is finalized by consensus.

Transcript hashing uses the following rules:

- All DKG transcripts are hashed using canonical DAG‑CBOR encoding.
- `DkgTranscriptCommit` binds `transcript_hash`, `prestate_hash`, and `operation_hash`.

Dealer packages (K2) follow these rules:

- Deterministic dealer packages are acceptable in trusted settings.
- Dealer packages must include encrypted shares for every participant.

BFT‑DKG (K3) follows these rules:

- A transcript is only usable once consensus finalizes the commit fact.
- All K3 ceremonies must reference the finalized transcript (hash or blob ref).

### 2.4 Layer 8: aura-testkit

The `aura-testkit` crate provides mock implementations for deterministic testing.

The mock handler lives in `crates/aura-testkit/src/stateful_effects/crypto.rs`. `MockCryptoHandler` uses a seed and counter for deterministic behavior, enabling reproducible test results, simulation of edge cases, and faster test execution.

## 3. Usage Boundary

Application code accesses cryptographic operations exclusively through effect traits. Direct imports of cryptographic libraries are forbidden outside Layer 3 handlers. Randomness flows through `RandomCoreEffects` for testability and simulation. See [Effects and Handlers Guide](802_effects_guide.md) for usage patterns and anti-patterns.

## 4. Allowed Locations

Direct cryptographic library usage is restricted to the following locations.

| Location | Allowed Libraries | Purpose |
|----------|-------------------|---------|
| `aura-core/src/crypto/*` | ed25519_dalek, frost_ed25519 | Type wrappers |
| `aura-core/src/types/authority.rs` | ed25519_dalek | Authority trait types |
| `aura-effects/src/*` | All crypto libs | Production handlers |
| `aura-effects/src/noise.rs` | snow | Noise Protocol implementation |
| `aura-testkit/*` | All crypto libs | Test infrastructure |
| `**/tests/*`, `*_test.rs` | OsRng | Test-only randomness |
| `#[cfg(test)]` modules | OsRng | Test-only randomness |

## 5. Security Invariants

The cryptographic architecture maintains these invariants.

1. All production crypto operations flow through `RealCryptoHandler`
2. Security review focuses on Layer 3 handlers, not scattered usage
3. All crypto is controllable via mock handlers for testing
4. Private keys remain in wrapper types, not exposed as raw bytes
5. Production randomness comes from OS entropy via `OsRng`
6. Identity and key bytes decoded from a remote payload are untrusted until checked against an authoritative local key source. A signature by the same key that signed an imported invitation proves continuity of that invitation, not trusted device identity.
7. A response is constructed from a completed signature over its canonical transcript; production response construction does not create an unsigned placeholder.
8. Imported authority tree operations require the verifying package and threshold policy of their parent epoch from an authenticated source. A matching parent commitment, a sibling transport session, or a key delivered beside the operation does not authenticate the operation. An enrolling device needs an explicit, ceremony-bound trust bootstrap before it can adopt a baseline tree. Required parent-policy reads distinguish absent records from storage and decoding failures. Participant inventory is complete and unique, and single-signer policy has exactly one participant and a threshold of one. An unreadable trusted record cannot authorize migration to a substitute verifier.
9. A transport receipt signed under a key carried in the receipt proves transcript integrity under that key. It does not authenticate the `AuthorityId` in the envelope. Promoting the source to verified authority or device identity requires comparison with an independently trusted key and an authenticated authority/device binding. A nonzero nonce is not replay protection without a checked replay state.

## 6. Signing Modes

Aura supports two signing modes to handle different account configurations.

### 6.1 SigningMode Enum

```rust
pub enum SigningMode {
    SingleSigner,  // Standard Ed25519 for 1-of-1
    Threshold,     // FROST for m-of-n where m >= 2
}
```

The `SingleSigner` mode is used for new user onboarding with single device accounts. It is also used for bootstrap scenarios before multi-device setup and for simple personal accounts that do not need threshold security.

The `Threshold` mode is used for multi-device accounts such as 2-of-3 or 3-of-5 configurations. It is also used for guardian-protected accounts and group decisions requiring multiple approvals.

### 6.2 Why Two Modes?

FROST requires at least 2 signers. For 1-of-1 configurations, we use standard Ed25519.

Ed25519 uses the same curve as FROST and produces compatible signatures for verification. Single signatures do not require nonce coordination or aggregation.

### 6.3 API Usage

The unified signing API selects between `SingleSigner` and `Threshold` modes based on the threshold parameter. For implementation patterns, see [Effects and Handlers Guide](802_effects_guide.md).

### 6.4 Storage Separation

Single-signer and threshold keys use separate storage paths managed by `SecureStorageEffects`. Path conventions are documented in [Effects and Handlers Guide](802_effects_guide.md).

## 7. FROST and Threshold Signatures

Aura provides a unified threshold signing architecture for all scenarios requiring m-of-n signatures where m >= 2.

### 7.1 Architecture Layers

The trait definition lives in `aura-core/src/effects/threshold.rs`.

```rust
#[async_trait]
pub trait ThresholdSigningEffects: Send + Sync {
    async fn bootstrap_authority(&self, authority: &AuthorityId) -> Result<PublicKeyPackage, ThresholdSigningError>;
    async fn sign(&self, context: SigningContext) -> Result<ThresholdSignature, ThresholdSigningError>;
    async fn threshold_config(&self, authority: &AuthorityId) -> Option<ThresholdConfig>;
    async fn threshold_state(&self, authority: &AuthorityId) -> Option<ThresholdState>;
    async fn has_signing_capability(&self, authority: &AuthorityId) -> bool;
    async fn public_key_package(&self, authority: &AuthorityId) -> Option<PublicKeyPackage>;
    async fn rotate_keys(&self, authority: &AuthorityId, new_threshold: u16, new_total_participants: u16, participants: &[ParticipantIdentity]) -> Result<(u64, Vec<Vec<u8>>, PublicKeyPackage), ThresholdSigningError>;
    async fn commit_key_rotation(&self, authority: &AuthorityId, new_epoch: u64) -> Result<(), ThresholdSigningError>;
    async fn rollback_key_rotation(&self, authority: &AuthorityId, failed_epoch: u64) -> Result<(), ThresholdSigningError>;
}
```

The trait provides methods for bootstrapping authorities, signing operations, querying configurations and state, checking capabilities, and key rotation lifecycle management.

Context types live in `aura-core/src/threshold/context.rs`.

```rust
pub struct SigningContext {
    pub authority: AuthorityId,
    pub operation: SignableOperation,
    pub approval_context: ApprovalContext,
}

pub enum SignableOperation {
    TreeOp(TreeOp),
    RecoveryApproval { target: AuthorityId, new_root: TreeCommitment },
    GroupProposal { group: AuthorityId, action: GroupAction },
    Message { domain: String, payload: Vec<u8> },
    OTAActivation { ceremony_id: [u8; 32], upgrade_hash: [u8; 32], prestate_hash: [u8; 32], activation_epoch: Epoch, ready: bool },
}

pub enum ApprovalContext {
    SelfOperation,
    RecoveryAssistance { recovering: AuthorityId, session_id: String },
    GroupDecision { group: AuthorityId, proposal_id: String },
    ElevatedOperation { operation_type: String, value_context: Option<String> },
}
```

The `SignableOperation` enum defines what is being signed. Its OTA activation variant should be interpreted as scoped activation approval evidence. `activation_epoch` is meaningful only when the chosen scope actually owns an epoch fence. The `ApprovalContext` enum provides context for audit and display purposes.

The service implementation lives in `aura-agent/src/runtime/services/threshold_signing.rs`. `ThresholdSigningService` manages per-authority signing state and key storage using `SecureStorageEffects` for key material persistence.

Bootstrap preserves an existing signing identity. Recovery loads the persisted
active epoch, policy, participants and public package instead of regenerating
epoch-zero keys. Invalid or incomplete persisted material is a recovery failure
and cannot authorize replacement keys. Decrypting an existing signing share is
a read-only operation on its wrapping key; a missing key or failed storage read
must not create new wrapping material. A setup code signed before recovery must
retain the same verifier afterward.

An enrollment response must verify under the exact provisional signing package,
epoch, mode and threshold policy selected by explicit user transfer. The
initiator retains that statement, its canonical digest and ceremony binding
before starting response owners; proof material supplied by the responder
cannot choose the expected verifier. Restored records retain the original
package and validity bounds. Missing legacy verifier state requires a new
transfer rather than self-certifying a response's key.

The setup validity interval governs admission to issuance. A successfully
retained ceremony may verify a response after setup expiry within the
invitation/ceremony's own response deadline; expiry does not substitute another
key or grant a new issuance. These deadlines are distinct from nonce consumption
and atomic activation, whose durable receipts remain separate requirements.

Historical setup-response signing is limited to a retained, admitted setup
request and its exact invitation/ceremony transcript. Possession of an old
encrypted key does not authorize signing after local revocation, cancellation
or completion. A response permit requires authenticated initiator-manifest
provenance, a durable setup admission receipt and current owner-controlled
lifecycle eligibility. It cannot accept an arbitrary payload, domain or epoch.
Setup admission must not infer validity from another device's exact wall-clock
timestamp; each owner's recorded admission evidence governs its validity gate.

For a threshold setup policy, response signing requires the authenticated
historical participant inventory and its quorum under the same narrow response
permit. Single-signer fallback cannot satisfy that policy. Retained shares remain
available while authorized pending requests need them; expiry, cancellation,
revocation and terminal outcomes govern retirement without removing active keys.

A retained local threshold share must match its authenticated signer identity,
threshold policy, group verifying key and verifying share. Its signing scalar
must derive that same verifying share, and the public package must contain the
exact authenticated participant inventory. Package decoding alone does not
establish these properties. Native FROST public packages do not encode a
threshold; a conversion default is not policy evidence. Membership in a prior
epoch cannot authorize signing after removal from the current participant set.

Initial signing-key readiness is distinct from bootstrap genesis completion.
A pending genesis record binds the authority, physical device, epoch-zero public
package digest and initialization phase before key persistence. Completion binds
the authenticated, durably indexed device-creation operation. A failed genesis
commit cannot publish a usable signing context. Recovery may resume creation
only from a matching pending initialization; legacy keys with missing or
conflicting creation evidence require explicit recovery rather than a new leaf.

Low-level primitives live in `aura-core/src/crypto/tree_signing.rs`. This module defines FROST types and pure coordination logic. It re-exports `frost_ed25519` types for type safety.

The handler in `aura-effects/src/crypto.rs` implements FROST key generation and signing. This is the only location with direct `frost_ed25519` library calls.

### 7.2 Serialized Size Invariants (FROST)

Aura treats the postcard serialization of FROST round-one data as canonical and fixed-size. This prevents malleability and makes invalid encodings unrepresentable at the boundary.

- `SigningNonces` (secret) **must serialize to exactly 138 bytes**
- `SigningCommitments` (public) **must serialize to exactly 69 bytes**

These sizes are enforced in `aura-core/src/crypto/tree_signing.rs` and mirrored in `aura-core/src/constants.rs`. See [Distributed Maintenance Guide](808_maintenance_guide.md) for update procedures when upstream encodings change.

### 7.3 Lifecycle Taxonomy (Key Generation vs Agreement)

Aura separates key generation from agreement/finality:

- **K1: Local/Single-Signer** (no DKG)
- **K2: Dealer-Based DKG** (trusted coordinator)
- **K3: Quorum/BFT-DKG** (consensus-finalized transcript)

Agreement modes are orthogonal:

- **A1: Provisional** (usable immediately, not final)
- **A2: Coordinator Soft-Safe** (bounded divergence + convergence cert)
- **A3: Consensus-Finalized** (unique, durable, non-forkable)

Leader selection (lottery/round seed/fixed coordinator) and pipelining are orthogonal optimizations, not agreement modes.

### 7.4 Usage Pattern

High-level signing operations use `AppCore` or direct `ThresholdSigningEffects` trait calls. See [Effects and Handlers Guide](802_effects_guide.md) for recommended patterns.

### 7.6 FROST Minimum Threshold

FROST requires `threshold >= 2`. Calling `frost_generate_keys(1, 1)` returns an error. For single-signer scenarios, use `generate_signing_keys(1, 1)` which routes to Ed25519 automatically.

## 8. Extensibility

The wrapper and trait abstraction enables algorithm migration and HSM integration without changing application code. Migration procedures are documented in [Distributed Maintenance Guide](808_maintenance_guide.md).

## See Also

- [Effect System](103_effect_system.md) for effect trait patterns
- [Project Structure](999_project_structure.md) for 8-layer architecture
- [Effects and Handlers Guide](802_effects_guide.md) for handler implementation guidance

## Independently transferred enrollment manifest

Enrollment baseline admission requires a signed `EnrollmentTrustManifest` and an independently transferred initiator verifier statement. The verifier statement binds the subject authority, physical initiator device, and confirmation key. A key embedded in an invitation or manifest cannot establish this pin. Decoding a transfer statement does not authorize admission; the explicit app transfer owner selects it.

The signed manifest binds the exact setup nonce and digest, reserved invitation and ceremony identifiers, actual provisional invitee authority and physical device, pending epoch, encrypted participant share, public package, canonical provisional threshold policy, and complete ordered baseline digest. Every attested baseline operation requires an exact parent epoch, commitment, signing node, group verifier, ordered participant inventory, threshold, signing mode, and agreement policy from that independently authenticated inventory. Replay checks each signature and reduction before any baseline/key mutation, rejects unused or missing inventory, and requires the exact final commitment. An epoch root key is not proof of an arbitrary node verifier.

Runtime admission rechecks the actual locally retained exported setup request and its signed policy bounds. The immutable secure admission record retains the selected verifier and original local admission time; recovery re-verifies its evidence rather than deserializing a trusted witness. Missing, corrupt, expired, mismatched, or legacy unbound evidence fails closed. Admission is a ceremony-scoped bootstrap permission, not authority adoption, durable peer membership, or permission to use retired signing shares. Acceptance binds the issuer-retained exact manifest digest in addition to the setup and ceremony binding.

#### Committed enrollment confirmation receipts

An invitee activation requires an issuer-pinned signed committed confirmation
bound to the exact independently admitted enrollment manifest, invitation,
ceremony, physical device and pending epoch. A cached accepted status, unsigned
confirmation or received signing package is insufficient. The committed
confirmation and original admitted budget acknowledgement are retained in an
immutable authenticated local receipt before activation.

Recovery verifies the original transfer signature, exact baseline and setup
binding, the actual committed confirmation signature, and the recorded original
budget state before producing activation evidence. Historical verification is
scoped to that retained receipt; it grants no historical signing authority and
cannot extend an admission window. Actual retained share, public package and
canonical provisional configuration must match the signed manifest. Generic
rotation activation cannot consume an enrollment import generation, and
activation cannot roll back a later retained epoch.

### Enrollment verifier origin enforcement

Retained enrollment response verification uses the independently retained setup verifier. Enrollment control verification uses the sealed independently admitted manifest. A remote key field and a trusted-looking local name do not establish either origin. Domain manifest signature verification establishes integrity under the explicitly independent input key and does not establish admission or current membership. Lexical test-only scopes do not relax production verifier contracts.

### Public enrollment configuration digest

The enrollment manifest's pending configuration digest is a `Hash32` content hash of the exact issued configuration encoding. It is public commitment material, not a threshold configuration or secret signing share. Its canonical encoding remains the original 32-byte array representation. Admission and generation validation compare this digest with freshly computed hashes of exact retained bytes; the digest alone confers no authority.

### Enrollment signing roster and authenticated topology

Enrollment keeps the original independently pinned signing quorum separate from the authenticated tree child topology. A signed AddLeaf changes observed child edges before the original signing key attests the RotateEpoch activation fence. Verification retains the exact signed quorum minimum and signer-roster upper bound while deriving topology cardinality solely from authenticated branch and leaf-parent edges. A verification projection does not materialize a canonical branch. A policy already materialized by authenticated tree operations remains an independent minimum; invalid or incompatible canonical policy metadata fails closed and requires an authenticated policy transition rather than a local repair. Missing node-specific verifier inventory cannot be replaced with the epoch root package.
