//! Layer 1: Cryptographic Domain Types
//!
//! Pure domain types and functions for threshold cryptography. Contains only
//! domain specification; implementations are effect-based and layered.
//!
//! **Core Components**:
//! - **Key Derivation (amp)**: Deterministic KDF (HKDF-SHA256) per docs/001_system_architecture.md
//! - **FROST Integration**: Domain types (Share, PartialSignature, ThresholdSignature)
//! - **Merkle Utilities**: Content-addressed tree operations for commitment verification
//! - **Ed25519**: Re-exports of ed25519-dalek for signature verification
//!
//! **Layered Implementation** (per docs/001_system_architecture.md):
//! - Basic crypto effects (sign, verify, key derive) → aura-effects/crypto (Layer 3)
//! - Multi-party FROST coordination → aura-protocol/guards (Layer 4)
//! - End-to-end FROST ceremonies → aura-frost (Layer 5)
//! - No effect handlers in this layer (pure functions/types only)

// Domain modules - pure functions and types only
pub mod amp;
pub mod key_derivation_types;
pub mod merkle;
pub mod tree_signing;

// Re-export commonly used cryptographic types
pub use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};

// Re-export key derivation types
pub use key_derivation_types::{IdentityKeyContext, KeyDerivationSpec, PermissionKeyContext};

// Re-export merkle utilities
pub use merkle::{
    build_commitment_tree, build_merkle_root, verify_merkle_proof, SimpleMerkleProof,
};

// Re-export tree signing utilities
pub use tree_signing::*;

// Create frost module for backwards compatibility with aura-frost crate
pub mod frost {
    //! FROST threshold cryptography compatibility module
    //!
    //! This module provides backwards compatibility for the aura-frost crate
    //! by re-exporting tree signing functionality under the frost namespace.

    pub use super::tree_signing;

    // Re-export specific types that aura-frost expects at the frost module level
    pub use super::tree_signing::{
        Nonce, NonceCommitment, PartialSignature, PublicKeyPackage, Share, SigningSession,
        ThresholdSignature, TreeSigningContext,
    };
}

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
