//! Cryptographic effects interface
//!
//! Pure trait definitions for cryptographic operations used by protocols.

use async_trait::async_trait;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

/// Cryptographic effects for protocol operations
#[async_trait]
pub trait CryptoEffects: Send + Sync {
    /// Generate random bytes
    async fn random_bytes(&self, len: usize) -> Vec<u8>;
    
    /// Generate 32 bytes of random data (commonly used)
    async fn random_bytes_32(&self) -> [u8; 32];
    
    /// Generate a random number in the given range
    async fn random_range(&self, range: std::ops::Range<u64>) -> u64;
    
    /// Hash data using Blake3
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32];
    
    /// Hash data using SHA-256
    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32];
    
    /// Sign data with Ed25519
    async fn ed25519_sign(&self, data: &[u8], key: &SigningKey) -> Result<Signature, CryptoError>;
    
    /// Verify Ed25519 signature
    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &Signature,
        public_key: &VerifyingKey,
    ) -> Result<bool, CryptoError>;
    
    /// Generate a new Ed25519 keypair
    async fn ed25519_generate_keypair(&self) -> Result<(SigningKey, VerifyingKey), CryptoError>;
    
    /// Derive public key from private key
    async fn ed25519_public_key(&self, private_key: &SigningKey) -> VerifyingKey;
    
    /// Constant-time comparison of byte arrays
    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool;
    
    /// Secure zero of memory
    fn secure_zero(&self, data: &mut [u8]);
}

/// Cryptographic errors
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("Invalid key format")]
    InvalidKey,
    
    #[error("Invalid signature format")]
    InvalidSignature,
    
    #[error("Signature verification failed")]
    VerificationFailed,
    
    #[error("Key generation failed: {reason}")]
    KeyGenerationFailed { reason: String },
    
    #[error("Insufficient entropy")]
    InsufficientEntropy,
    
    #[error("Cryptographic operation failed: {operation}")]
    OperationFailed { operation: String },
    
    #[error("Backend error: {source}")]
    Backend { source: Box<dyn std::error::Error + Send + Sync> },
}