# Cryptographic Architecture

This document describes the cryptographic architecture in Aura. It defines layer responsibilities, code organization patterns, security invariants, and compliance requirements for cryptographic operations.

## 1. Overview

Aura's cryptographic architecture follows the 8-layer system design with strict separation of concerns:

- **Layer 1 (aura-core)**: Type wrappers, trait definitions, pure functions
- **Layer 3 (aura-effects)**: Production implementations with real crypto libraries
- **Layer 8 (aura-testkit)**: Mock implementations for deterministic testing

This separation ensures that cryptographic operations are auditable, testable, and maintainable. Security review focuses on a small number of files rather than scattered usage throughout the codebase.

## 2. Layer Responsibilities

### 2.1 Layer 1: aura-core

`aura-core` provides cryptographic foundations without direct side effects.

**Type Wrappers** (`crates/aura-core/src/crypto/ed25519.rs`):

```rust
pub struct Ed25519SigningKey { ... }
pub struct Ed25519VerifyingKey { ... }
pub struct Ed25519Signature { ... }
```

These wrappers:
- Delegate to `ed25519_dalek` internally
- Expose a stable API independent of the underlying library
- Enable future algorithm migration without changing application code
- Provide type safety across crate boundaries

**Effect Trait Definitions** (`crates/aura-core/src/effects/`):

```rust
#[async_trait]
pub trait CryptoEffects {
    async fn sign(&self, key: &Ed25519SigningKey, data: &[u8]) -> Ed25519Signature;
    async fn verify(&self, key: &Ed25519VerifyingKey, data: &[u8], sig: &Ed25519Signature) -> bool;
    async fn hash(&self, data: &[u8]) -> [u8; 32];
}

#[async_trait]
pub trait RandomEffects {
    async fn random_bytes(&self, len: usize) -> Vec<u8>;
}
```

**Pure Functions** (`crates/aura-core/src/crypto/`):

Hash functions, signature verification, and other deterministic operations may be implemented as pure functions when no side effects are involved.

### 2.2 Layer 3: aura-effects

`aura-effects` contains the **only** production implementations that directly use cryptographic libraries.

**Production Handler** (`crates/aura-effects/src/crypto.rs`):

```rust
pub struct RealCryptoHandler;

impl CryptoEffects for RealCryptoHandler {
    async fn sign(&self, key: &Ed25519SigningKey, data: &[u8]) -> Ed25519Signature {
        // Uses ed25519_dalek directly
    }
}
```

**Allowed Direct Imports** in Layer 3:
- `ed25519_dalek`
- `frost_ed25519`
- `chacha20poly1305`
- `getrandom`
- `rand_core::OsRng`

### 2.3 Layer 8: aura-testkit

`aura-testkit` provides mock implementations for deterministic testing.

**Mock Handler** (`crates/aura-testkit/src/mock_effects.rs`):

```rust
pub struct MockCryptoHandler {
    seed: u64,
}

impl CryptoEffects for MockCryptoHandler {
    async fn sign(&self, key: &Ed25519SigningKey, data: &[u8]) -> Ed25519Signature {
        // Deterministic signing for reproducible tests
    }
}
```

Mock handlers enable:
- Reproducible test results
- Simulation of edge cases
- Faster test execution

## 3. Code Organization Patterns

### 3.1 Correct Usage

**Application code** should use effect traits:

```rust
// CORRECT: Using effect trait
async fn authenticate<E: CryptoEffects>(effects: &E, key: &Ed25519SigningKey, data: &[u8]) {
    let signature = effects.sign(key, data).await;
    // ...
}
```

**Application code** should use aura-core wrappers:

```rust
// CORRECT: Using aura-core wrappers
use aura_core::crypto::ed25519::{Ed25519SigningKey, Ed25519VerifyingKey};

fn verify_authority(key: &Ed25519VerifyingKey, data: &[u8], sig: &Ed25519Signature) -> bool {
    key.verify(data, sig).is_ok()
}
```

### 3.2 Incorrect Usage

**Do not** import crypto libraries directly in application code:

```rust
// INCORRECT: Direct crypto library import
use ed25519_dalek::{SigningKey, VerifyingKey};  // BAD

// INCORRECT: Direct randomness
use rand_core::OsRng;  // BAD (outside Layer 3)
let mut rng = OsRng;
```

### 3.3 Randomness Patterns

All randomness should flow through `RandomEffects`:

```rust
// CORRECT: Effect-based randomness
async fn generate_nonce<E: RandomEffects>(effects: &E) -> [u8; 12] {
    let bytes = effects.random_bytes(12).await;
    bytes.try_into().unwrap()
}
```

For encryption in feature crates (Layer 5), use parameter injection:

```rust
// CORRECT: Randomness passed as parameter
pub struct EncryptionRandomness {
    nonce: [u8; 12],
    padding: Vec<u8>,
}

pub fn encrypt_with_randomness(data: &[u8], key: &[u8], randomness: EncryptionRandomness) -> Vec<u8> {
    // Deterministic given the randomness parameter
}
```

## 4. Allowed Locations

The following locations may directly use cryptographic libraries:

| Location | Allowed Libraries | Purpose |
|----------|-------------------|---------|
| `aura-core/src/crypto/*` | ed25519_dalek | Type wrappers |
| `aura-core/src/types/authority.rs` | ed25519_dalek | Authority trait types |
| `aura-effects/src/*` | All crypto libs | Production handlers |
| `aura-testkit/*` | All crypto libs | Test infrastructure |
| `**/tests/*`, `*_test.rs` | OsRng | Test-only randomness |
| `#[cfg(test)]` modules | OsRng | Test-only randomness |

## 5. Architecture Compliance

### 5.1 Automated Enforcement

Architecture compliance is enforced via `scripts/check-arch.sh`:

```bash
# Run crypto-specific checks
just check-arch --crypto

# Run all architecture checks (includes crypto)
just check-arch

# Quick mode (includes crypto)
just check-arch --quick
```

The crypto checks verify:
1. No `use ed25519_dalek` outside allowed locations
2. No `OsRng` usage outside effect handlers and tests
3. No `getrandom::` usage outside effect handlers

### 5.2 Security Review Checklist

When adding new cryptographic code:

- [ ] Does it use effect traits rather than direct library calls?
- [ ] Is randomness obtained through `RandomEffects`?
- [ ] Are keys wrapped in `aura-core` types?
- [ ] Does `just check-arch --crypto` pass?
- [ ] Is the code in an appropriate layer?

### 5.3 Adding New Crypto Operations

1. **Define the operation** in `aura-core/src/effects/crypto.rs` (trait method)
2. **Implement production** in `aura-effects/src/crypto.rs`
3. **Implement mock** in `aura-testkit/src/mock_effects.rs`
4. **Verify compliance** with `just check-arch --crypto`

## 6. Security Invariants

The cryptographic architecture maintains these invariants:

1. **Single Source of Truth**: All production crypto operations flow through `RealCryptoHandler`
2. **Auditable Surface**: Security review focuses on Layer 3 handlers, not scattered usage
3. **Testability**: All crypto is controllable via mock handlers
4. **Key Isolation**: Private keys remain in wrapper types, not exposed as raw bytes
5. **Randomness Quality**: Production randomness comes from OS entropy via `OsRng`

## 7. FROST and Threshold Signatures

FROST threshold signatures use a different pattern from standard Ed25519:

**Primitives** (`aura-core/src/crypto/tree_signing.rs`):
- Defines FROST types and pure coordination logic
- No direct crypto library imports (uses frost_ed25519 types)

**Handler** (`aura-effects/src/crypto.rs`):
- Implements FROST key generation and signing
- Only location with direct `frost_ed25519` usage

**YAGNI Note**: We do not abstract FROST behind a generic trait because:
- Only one threshold signature scheme is needed
- Direct usage is clearer than premature abstraction
- See `crates/aura-core/src/crypto/README.md` for details

## 8. Future Considerations

### 8.1 Algorithm Migration

The wrapper pattern enables algorithm migration:

1. Update wrappers in `aura-core/src/crypto/`
2. Update handler in `aura-effects/src/crypto.rs`
3. Application code remains unchanged

### 8.2 HSM Integration

Hardware Security Module support would require:

1. New `HsmCryptoHandler` implementing `CryptoEffects`
2. Runtime selection between `RealCryptoHandler` and `HsmCryptoHandler`
3. No changes to application code

## 9. References

- [Effect System](./106_effect_system_and_runtime.md) - Effect trait patterns
- [Project Structure](./999_project_structure.md) - 8-layer architecture
- [Development Patterns](./805_development_patterns.md) - Code location guidance
- [Crypto Consolidation Plan](../work/crypto.md) - Implementation details
