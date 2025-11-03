//! Aura Crypto: Middleware-based cryptographic operations
//!
//! This crate provides a middleware-based cryptographic operations system that follows
//! the foundation pattern established throughout Aura. All cryptographic operations
//! are processed through configurable middleware stacks that provide security,
//! monitoring, and compliance features.
//!
//! ## Core Components
//!
//! - **Middleware System**: Type-safe middleware composition for crypto operations
//! - **Security Levels**: Hierarchical security enforcement (Basic → Critical)
//! - **Operation Tracking**: Comprehensive audit logging and monitoring
//! - **Hardware Integration**: TEE/HSM support with attestation
//! - **Performance Optimization**: Intelligent caching and rate limiting
//!
//! ## Essential Crypto Library Components
//!
//!
//! All cryptographic operations are now provided through the composable middleware system
//! which provides security, monitoring, and compliance features.

#![allow(clippy::result_large_err)]

// Merkle tree utilities
pub mod merkle;

// Middleware system (complete implementation)
pub mod middleware;

// Re-export commonly used types for convenience
pub use ed25519_dalek::{Signature as Ed25519Signature, VerifyingKey as Ed25519VerifyingKey};

// Error types - unified error system
pub use aura_types::{AuraError, AuraResult, ErrorCode, ErrorSeverity};
/// General cryptographic operation error alias
pub type CryptoError = AuraError;
/// Crypto result type alias
pub type Result<T> = AuraResult<T>;

// Re-export complete middleware system
pub use middleware::*;

// Re-export merkle utilities
pub use merkle::{
    build_commitment_tree, build_merkle_root, verify_merkle_proof, SimpleMerkleProof,
};

// Type aliases for HPKE functionality (placeholder implementations)
pub type HpkePrivateKey = [u8; 32];
pub type HpkePublicKey = [u8; 32];
pub type MerkleProof = SimpleMerkleProof;

// FROST threshold cryptography types (re-exported for compatibility)
pub use frost_ed25519::{
    keys::{KeyPackage as FrostKeyPackage, PublicKeyPackage as FrostPublicKeyPackage},
    round1::SigningCommitments as FrostSigningCommitments,
    round2::SignatureShare as FrostSignatureShare,
    Signature as FrostSignature,
};

/// Legacy KeyShare type for agent compatibility
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyShare {
    pub identifier: u16,
    pub key_package: Vec<u8>, // Serialized FrostKeyPackage
    pub public_key_package: Vec<u8>, // Serialized FrostPublicKeyPackage
}

impl KeyShare {
    /// Create a new KeyShare from FROST components
    pub fn new(identifier: u16, key_package: Vec<u8>, public_key_package: Vec<u8>) -> Self {
        Self {
            identifier,
            key_package,
            public_key_package,
        }
    }
    
    /// Get the identifier
    pub fn identifier(&self) -> u16 {
        self.identifier
    }
    
    /// Get the key package bytes
    pub fn key_package(&self) -> &[u8] {
        &self.key_package
    }
    
    /// Get the public key package bytes
    pub fn public_key_package(&self) -> &[u8] {
        &self.public_key_package
    }
}

/// Generate a UUID for compatibility
pub fn generate_uuid() -> uuid::Uuid {
    uuid::Uuid::new_v4()
}

/// Simple HPKE key pair implementation (placeholder)
#[derive(Debug, Clone)]
pub struct HpkeKeyPair {
    pub private_key: HpkePrivateKey,
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
    public_key.verify(message, signature).map_err(|e| {
        AuraError::crypto_operation_failed(format!("Signature verification failed: {}", e))
    })
}
