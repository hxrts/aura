//! Aura Crypto: Middleware-based cryptographic operations
//!
//! This crate provides a middleware-based cryptographic operations system that follows
//! the foundation pattern established throughout Aura. All cryptographic operations
//! are processed through configurable middleware stacks that provide security,
//! monitoring, and compliance features.
//!
//! ## Core Components
//!
//! - **Effects System**: Clean abstraction layer for cryptographic operations
//! - **Middleware System**: TODO fix - Simplified validation and parameter checking
//! - **FROST Integration**: Re-exported types for compatibility (see aura-frost crate)
//! - **Merkle Utilities**: Tree operations for commitment verification
//!
//! ## TODO fix - Simplified Architecture
//!
//! The crypto crate now focuses on essential functionality with validation middleware.
//! Complex features like audit logging, hardware security, and caching have been
//! removed or moved to appropriate layers (effects system, choreographic protocols).

#![allow(clippy::result_large_err)]

// Effects imported from aura-core
// (effects.rs was deleted to eliminate duplication)

// Key derivation system
pub mod key_derivation;

// Merkle tree utilities
pub mod merkle;

// Middleware system (complete implementation)
pub mod middleware;

// FROST threshold signing primitives
pub mod frost;

// Re-export commonly used types for convenience
pub use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};

// Error types - unified error system
pub use aura_core::{AuraError, Result as AuraResult};
/// General cryptographic operation error alias
pub type CryptoError = AuraError;
/// Crypto result type alias
pub type Result<T> = AuraResult<T>;

// Re-export complete middleware system
pub use middleware::*;

// Re-export effects system from aura-core
pub use aura_core::effects::{CryptoEffects, TimeEffects};

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
    uuid::Uuid::new_v4()
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
) -> Result<()> {
    use ed25519_dalek::Verifier;
    public_key
        .verify(message, signature)
        .map_err(|e| AuraError::crypto(format!("Signature verification failed: {}", e)))
}
