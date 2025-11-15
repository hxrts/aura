//! Cryptographic Domain Types and Utilities
//!
//! This module provides domain-specific cryptographic types and pure functions
//! for use throughout the Aura system. Following the 8-layer architecture,
//! these are **Layer 1 (Foundation)** components that contain only domain logic
//! and semantic definitions - no effect handlers or implementations.
//!
//! ## Core Domain Components
//!
//! - **Key Derivation**: Pure functions for deterministic key derivation (DKD)
//! - **FROST Integration**: Domain types for threshold cryptography
//! - **Merkle Utilities**: Tree operations for commitment verification
//! - **Standard Crypto Types**: Re-exports of ed25519-dalek types
//!
//! ## Architecture
//!
//! This module contains **domain specification only**:
//! - Pure functions with no side effects
//! - Domain types and data structures
//! - Mathematical operations and algorithms
//! - No effect handlers, middleware, or runtime implementations
//!
//! Effect implementations live in appropriate layers:
//! - Basic crypto operations → `aura-effects` (Layer 3)
//! - Multi-party coordination → `aura-protocol` (Layer 4)
//! - Complete FROST ceremonies → `aura-frost` (Layer 5)

// Domain modules - pure functions and types only
pub mod frost;
pub mod key_derivation;
pub mod merkle;

// Re-export commonly used cryptographic types
pub use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};

// Re-export key derivation functions and types
pub use key_derivation::{
    derive_encryption_key, derive_key_material, IdentityKeyContext, KeyDerivationSpec,
    PermissionKeyContext,
};

// Re-export merkle utilities
pub use merkle::{
    build_commitment_tree, build_merkle_root, verify_merkle_proof, SimpleMerkleProof,
};

/// HPKE private key - 32 bytes for X25519
pub type HpkePrivateKey = [u8; 32];
/// HPKE public key - 32 bytes for X25519
pub type HpkePublicKey = [u8; 32];
/// Merkle proof using simple node hashes
pub type MerkleProof = SimpleMerkleProof;

/// Generate a UUID for compatibility
///
/// This is a basic utility function. For more sophisticated effects-based UUID generation,
/// use the RandomEffects trait implementations from aura-effects.
#[allow(clippy::disallowed_methods)]
pub fn generate_uuid() -> uuid::Uuid {
    uuid::Uuid::from_bytes([0u8; 16])
}

/// Simple HPKE key pair implementation (placeholder)
#[derive(Debug, Clone)]
pub struct HpkeKeyPair {
    /// Private key for decryption (X25519)
    pub private_key: HpkePrivateKey,
    /// Public key for encryption (X25519)
    pub public_key: HpkePublicKey,
}

impl HpkeKeyPair {
    /// Generate a new HPKE key pair using a random number generator
    pub fn generate<R: rand::RngCore>(rng: &mut R) -> Self {
        let mut private_key = [0u8; 32];
        let mut public_key = [0u8; 32];
        rng.fill_bytes(&mut private_key);
        rng.fill_bytes(&mut public_key);

        Self {
            private_key,
            public_key,
        }
    }

    /// Get the private key
    pub fn private_key(&self) -> &HpkePrivateKey {
        &self.private_key
    }

    /// Get the public key
    pub fn public_key(&self) -> &HpkePublicKey {
        &self.public_key
    }
}

/// Verify an Ed25519 signature using a public key
///
/// This is a convenience function that verifies a signature against a message
/// using the provided public key. Works for both regular Ed25519 signatures
/// and FROST threshold signatures (which produce standard Ed25519 signatures).
pub fn ed25519_verify(
    public_key: &Ed25519VerifyingKey,
    message: &[u8],
    signature: &Ed25519Signature,
) -> crate::AuraResult<()> {
    use ed25519_dalek::Verifier;
    public_key
        .verify(message, signature)
        .map_err(|e| crate::AuraError::crypto(format!("Signature verification failed: {}", e)))
}