# Cryptographic Architecture

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

Type wrappers live in `crates/aura-core/src/crypto/ed25519.rs`.

```rust
pub struct Ed25519SigningKey(pub Vec<u8>);
pub struct Ed25519VerifyingKey(pub Vec<u8>);
pub struct Ed25519Signature(pub Vec<u8>);
```

These wrappers delegate to `ed25519_dalek` internally. They expose a stable API independent of the underlying library. They enable future algorithm migration without changing application code. They provide type safety across crate boundaries.

Effect trait definitions live in `crates/aura-core/src/effects/`. The `CryptoCoreEffects` trait inherits from `RandomCoreEffects` and provides cryptographic operations.

```rust
#[async_trait]
pub trait CryptoCoreEffects: RandomCoreEffects + Send + Sync {
    // Key derivation
    async fn hkdf_derive(&self, ikm: &[u8], salt: &[u8], info: &[u8], output_len: u32) -> Result<Vec<u8>, CryptoError>;
    async fn derive_key(&self, master_key: &[u8], context: &KeyDerivationContext) -> Result<Vec<u8>, CryptoError>;

    // Ed25519 signatures
    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError>;
    async fn ed25519_sign(&self, message: &[u8], private_key: &[u8]) -> Result<Vec<u8>, CryptoError>;
    async fn ed25519_verify(&self, message: &[u8], signature: &[u8], public_key: &[u8]) -> Result<bool, CryptoError>;

    // Unified signing API
    async fn generate_signing_keys(&self, threshold: u16, max_signers: u16) -> Result<SigningKeyGenResult, CryptoError>;
    async fn sign_with_key(&self, message: &[u8], key_package: &[u8], mode: SigningMode) -> Result<Vec<u8>, CryptoError>;
    async fn verify_signature(&self, message: &[u8], signature: &[u8], public_key_package: &[u8], mode: SigningMode) -> Result<bool, CryptoError>;

    // FROST threshold signatures
    async fn frost_generate_keys(&self, threshold: u16, max_signers: u16) -> Result<FrostKeyGenResult, CryptoError>;
    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError>;
    async fn frost_sign_share(&self, signing_package: &FrostSigningPackage, key_share: &[u8], nonces: &[u8]) -> Result<Vec<u8>, CryptoError>;
    async fn frost_aggregate_signatures(&self, signing_package: &FrostSigningPackage, signature_shares: &[Vec<u8>]) -> Result<Vec<u8>, CryptoError>;
    async fn frost_verify(&self, message: &[u8], signature: &[u8], group_public_key: &[u8]) -> Result<bool, CryptoError>;

    // Symmetric encryption
    async fn chacha20_encrypt(&self, plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    async fn chacha20_decrypt(&self, ciphertext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    async fn aes_gcm_encrypt(&self, plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;
    async fn aes_gcm_decrypt(&self, ciphertext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>, CryptoError>;

    // Utility methods
    fn is_simulated(&self) -> bool;
    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool;
    fn secure_zero(&self, data: &mut [u8]);
}
```

The trait provides key derivation, Ed25519 signatures, unified signing that routes between single-signer and threshold modes, FROST threshold operations, and symmetric encryption. Hashing is not included because it is a pure operation. Use `aura_core::hash::hash()` for synchronous hashing instead.

The `RandomCoreEffects` trait provides cryptographically secure random number generation.

```rust
#[async_trait]
pub trait RandomCoreEffects: Send + Sync {
    async fn random_bytes(&self, len: usize) -> Vec<u8>;
    async fn random_bytes_32(&self) -> [u8; 32];
    async fn random_u64(&self) -> u64;
    async fn random_range(&self, min: u64, max: u64) -> u64;
    async fn random_uuid(&self) -> Uuid;
}
```

The trait provides methods for generating random bytes, fixed-size arrays, integers, ranges, and UUIDs. All randomness flows through this trait for testability and simulation.

Pure functions in `crates/aura-core/src/crypto/` implement hash functions, signature verification, and other deterministic operations. These require no side effects and can be called directly.

### 2.2 Layer 3: aura-effects

The `aura-effects` crate contains the only production implementations that directly use cryptographic libraries.

The production handler lives in `crates/aura-effects/src/crypto.rs`.

```rust
pub struct RealCryptoHandler {
    seed: Option<[u8; 32]>,
}

impl RealCryptoHandler {
    pub fn new() -> Self { Self { seed: None } }
    pub fn seeded(seed: [u8; 32]) -> Self { Self { seed: Some(seed) } }
}

impl CryptoCoreEffects for RealCryptoHandler {
    async fn ed25519_sign(&self, message: &[u8], private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        // Uses ed25519_dalek directly
    }
    // ... other methods
}
```

The handler can operate with OS entropy for production or with a seed for deterministic testing. It implements all methods from `CryptoCoreEffects` and `RandomCoreEffects`.

The following direct imports are allowed in Layer 3:

- `ed25519_dalek`
- `frost_ed25519`
- `chacha20poly1305`
- `aes_gcm`
- `getrandom`
- `rand_core::OsRng`
- `rand_chacha`
- `hkdf`

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

The mock handler lives in `crates/aura-testkit/src/stateful_effects/crypto.rs`.

```rust
pub struct MockCryptoHandler {
    seed: u64,
    counter: Arc<Mutex<u64>>,
}

impl MockCryptoHandler {
    pub fn new() -> Self { Self { seed: 42, counter: Arc::new(Mutex::new(0)) } }
    pub fn with_seed(seed: u64) -> Self { Self { seed, counter: Arc::new(Mutex::new(0)) } }
}

impl CryptoCoreEffects for MockCryptoHandler {
    async fn ed25519_sign(&self, message: &[u8], private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        // Deterministic signing for reproducible tests
    }
}
```

The mock handler uses a seed and counter for deterministic behavior. This enables reproducible test results, simulation of edge cases, and faster test execution.

## 3. Code Organization Patterns

### 3.1 Correct Usage

Application code should use effect traits.

```rust
async fn authenticate<E: CryptoCoreEffects>(effects: &E, private_key: &[u8], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    effects.ed25519_sign(data, private_key).await
}
```

This pattern ensures all cryptographic operations flow through the effect system. The generic constraint allows both production and mock implementations.

Application code should use aura-core wrappers for type safety.

```rust
use aura_core::crypto::ed25519::{Ed25519SigningKey, Ed25519VerifyingKey, Ed25519Signature};

fn verify_authority(key: &Ed25519VerifyingKey, data: &[u8], sig: &Ed25519Signature) -> Result<(), AuraError> {
    key.verify(data, sig)
}
```

The wrapper types provide a stable API and enable algorithm migration without changing application code.

### 3.2 Incorrect Usage

Do not import crypto libraries directly in application code.

```rust
// INCORRECT: Direct crypto library import
use ed25519_dalek::{SigningKey, VerifyingKey};  // BAD

// INCORRECT: Direct randomness
use rand_core::OsRng;  // BAD (outside Layer 3)
let mut rng = OsRng;
```

Direct imports bypass the effect system and break testability. They also scatter cryptographic usage throughout the codebase.

### 3.3 Randomness Patterns

All randomness should flow through `RandomCoreEffects`.

```rust
async fn generate_nonce<E: RandomCoreEffects>(effects: &E) -> [u8; 12] {
    let bytes = effects.random_bytes(12).await;
    bytes.try_into().expect("12 bytes")
}
```

For encryption in feature crates, use parameter injection.

```rust
pub struct EncryptionRandomness {
    nonce: [u8; 12],
    padding: Vec<u8>,
}

pub fn encrypt_with_randomness(data: &[u8], key: &[u8], randomness: EncryptionRandomness) -> Vec<u8> {
    // Deterministic given the randomness parameter
}
```

This pattern enables deterministic testing by externalizing randomness.

## 4. Allowed Locations

The following locations may directly use cryptographic libraries.

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

FROST mathematically requires at least 2 signers because threshold signatures need multiple parties. For 1-of-1 configurations, we use standard Ed25519.

Ed25519 uses the same curve as FROST and produces compatible signatures for verification. It has no protocol overhead such as nonce coordination or aggregation. It is simpler and faster for the single-signer case.

### 6.3 API Usage

The unified API handles mode selection automatically.

```rust
// Generate keys - mode is determined by threshold
let keys = crypto.generate_signing_keys(threshold, max_signers).await?;
// keys.mode == SingleSigner if (1, 1), Threshold otherwise

// Sign with the key package (single-signer only)
let signature = crypto.sign_with_key(message, &key_package, keys.mode).await?;

// Verify the signature
let valid = crypto.verify_signature(message, &signature, &keys.public_key_package, keys.mode).await?;
```

For threshold mode where m >= 2, the `sign_with_key()` method returns an error. Threshold signing requires the full FROST protocol flow with nonce coordination across signers.

### 6.4 Storage Separation

Single-signer and threshold keys use different storage paths.

```
signing_keys/<authority>/<epoch>/1       # SingleSignerKeyPackage (Ed25519)
frost_keys/<authority>/<epoch>/<index>   # FROST KeyPackage
```

The storage location is managed by `SecureStorageEffects`. The `authority` is the `AuthorityId` in display format. The `epoch` is the current key epoch. The `index` is the signer index for FROST keys.

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
    async fn has_signing_capability(&self, authority: &AuthorityId) -> bool;
    async fn public_key_package(&self, authority: &AuthorityId) -> Option<PublicKeyPackage>;
}
```

The trait provides methods for bootstrapping authorities, signing operations, querying configurations, and checking capabilities.

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
}

pub enum ApprovalContext {
    SelfOperation,
    RecoveryAssistance { recovering: AuthorityId, session_id: String },
    GroupDecision { group: AuthorityId, proposal_id: String },
    ElevatedOperation { operation_type: String, value_context: Option<String> },
}
```

The `SignableOperation` enum defines what is being signed. The `ApprovalContext` enum provides context for audit and display purposes.

The service implementation lives in `aura-agent/src/runtime/services/threshold_signing.rs`.

```rust
pub struct ThresholdSigningService {
    effects: Arc<RwLock<AuraEffectSystem>>,
    contexts: RwLock<HashMap<AuthorityId, SigningContextState>>,
}
```

The service manages per-authority signing state and key storage. It uses `SecureStorageEffects` for key material persistence.

Low-level primitives live in `aura-core/src/crypto/tree_signing.rs`. This module defines FROST types and pure coordination logic. It re-exports `frost_ed25519` types for type safety.

The handler in `aura-effects/src/crypto.rs` implements FROST key generation and signing. This is the only location with direct `frost_ed25519` library calls.

### 7.2 Serialized Size Invariants (FROST)

Aura treats the postcard serialization of FROST round-one data as **canonical and fixed-size**. This prevents malleability and makes invalid encodings unrepresentable at the boundary.

- `SigningNonces` (secret) **must serialize to exactly 138 bytes**
- `SigningCommitments` (public) **must serialize to exactly 69 bytes**

These sizes are enforced in `aura-core/src/crypto/tree_signing.rs` and mirrored in `aura-core/src/constants.rs`. If the upstream FROST or postcard encoding changes, update the constants and add/adjust tests to lock in the new canonical sizes.

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

The recommended pattern uses `AppCore` for high-level operations.

```rust
// Sign a tree operation
let attested_op = app_core.sign_tree_op(&tree_op).await?;

// Bootstrap 1-of-1 keys for single-device accounts (uses Ed25519)
let public_key = app_core.bootstrap_signing_keys().await?;
```

For direct trait usage, import and call the service.

```rust
use aura_core::effects::ThresholdSigningEffects;

let context = SigningContext::self_tree_op(authority, tree_op);
let signature = signing_service.sign(context).await?;
```

### 7.5 Design Rationale

The unified trait abstraction enables a consistent interface across multi-device, guardian, and group scenarios. It provides proper audit context via `ApprovalContext` for UX and logging. It enables testability via mock implementations in `aura-testkit`. It provides key isolation with secure storage integration.

### 7.6 FROST Minimum Threshold

FROST requires `threshold >= 2`. Calling `frost_generate_keys(1, 1)` returns an error. For single-signer scenarios, use `generate_signing_keys(1, 1)` which routes to Ed25519 automatically.

## 8. Future Considerations

### 8.1 Algorithm Migration

The wrapper pattern enables algorithm migration.

1. Update wrappers in `aura-core/src/crypto/`
2. Update handler in `aura-effects/src/crypto.rs`
3. Application code remains unchanged

### 8.2 HSM Integration

Hardware Security Module support would require a new `HsmCryptoHandler` implementing `CryptoCoreEffects`. Runtime selection between `RealCryptoHandler` and `HsmCryptoHandler` would be needed. Application code would require no changes.

## See Also

- [Effect System and Runtime](106_effect_system_and_runtime.md) for effect trait patterns
- [Project Structure](999_project_structure.md) for 8-layer architecture
- [Development Patterns and Workflows](805_development_patterns.md) for code location guidance
