//! Cryptographic effects trait definitions
//!
//! This module defines the trait interfaces for cryptographic operations.
//! Actual cryptographic implementations are provided by aura-crypto crate.
//! Effect handlers that integrate aura-crypto are provided by aura-protocol handlers.

use async_trait::async_trait;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use aura_types::AuraError;

/// Cryptographic operation error
pub type CryptoError = AuraError;

/// Cryptographic effects interface
/// 
/// This trait defines cryptographic operations for the Aura effects system.
/// The actual cryptographic primitives are implemented in aura-crypto crate.
/// Effect handlers are implemented in aura-protocol handlers for different environments:
/// - Production: Real cryptographic operations using aura-crypto
/// - Testing: Deterministic mock operations  
/// - Simulation: Controlled cryptographic scenarios
#[async_trait]
pub trait CryptoEffects: Send + Sync {
    /// BLAKE3 hash function
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32];
    
    /// SHA256 hash function (for compatibility)
    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32];
    
    /// Generate cryptographically secure random bytes
    async fn random_bytes(&self, len: usize) -> Vec<u8>;
    
    /// Generate 32 random bytes as array
    async fn random_bytes_32(&self) -> [u8; 32];
    
    /// Generate random number in range
    async fn random_range(&self, range: std::ops::Range<u64>) -> u64;
    
    /// Ed25519 signature generation
    async fn ed25519_sign(&self, data: &[u8], key: &SigningKey) -> Result<Signature, CryptoError>;
    
    /// Ed25519 signature verification
    async fn ed25519_verify(
        &self, 
        data: &[u8], 
        signature: &Signature, 
        public_key: &VerifyingKey
    ) -> Result<bool, CryptoError>;
    
    /// Generate Ed25519 key pair
    async fn ed25519_generate_keypair(&self) -> Result<(SigningKey, VerifyingKey), CryptoError>;
    
    /// Get public key from private key
    async fn ed25519_public_key(&self, private_key: &SigningKey) -> VerifyingKey;
    
    /// Constant-time comparison
    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool;
    
    /// Securely zero memory
    fn secure_zero(&self, data: &mut [u8]);
}