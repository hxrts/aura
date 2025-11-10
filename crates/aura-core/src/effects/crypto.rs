//! Cryptographic effects trait definitions
//!
//! This module defines the trait interfaces for cryptographic operations.
//! Actual cryptographic implementations are provided by aura-crypto crate.
//! Effect handlers that integrate aura-crypto are provided by aura-protocol handlers.

use super::RandomEffects;
use crate::{AccountId, AuraError, DeviceId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Cryptographic operation error
pub type CryptoError = AuraError;

/// Key derivation context for deterministic key generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationContext {
    /// Application identifier
    pub app_id: String,
    /// Context string for key derivation
    pub context: String,
    /// Derivation path components
    pub derivation_path: Vec<u32>,
    /// Account identifier
    pub account_id: AccountId,
    /// Device identifier
    pub device_id: DeviceId,
}

/// FROST signing package for threshold signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostSigningPackage {
    /// Message to be signed
    pub message: Vec<u8>,
    /// Signing package data
    pub package: Vec<u8>,
    /// Participant identifiers
    pub participants: Vec<u16>,
}

/// Cryptographic effects interface
///
/// This trait defines cryptographic operations for the Aura effects system.
/// The actual cryptographic primitives are implemented in aura-crypto crate.
/// Effect handlers are implemented in aura-protocol handlers for different environments:
/// - Production: Real cryptographic operations using aura-crypto
/// - Testing: Deterministic mock operations
/// - Simulation: Controlled cryptographic scenarios
///
/// CryptoEffects inherits from RandomEffects to ensure all randomness is deterministic
/// and controllable for simulation purposes.
#[async_trait]
pub trait CryptoEffects: RandomEffects + Send + Sync {
    // ====== Hash Functions ======

    /// Unified hash function using SHA256
    ///
    /// This is the primary hashing method used throughout Aura. It uses SHA256
    /// uniformly across the system for content addressing, fingerprinting, and
    /// cryptographic commitments.
    async fn hash(&self, data: &[u8]) -> [u8; 32];

    /// HMAC using SHA256
    ///
    /// Computes HMAC-SHA256 for authenticated hashing with a key.
    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32];

    // Note: Random methods (random_bytes, random_bytes_32, random_range)
    // are inherited from RandomEffects trait

    // ====== Key Derivation ======

    /// HKDF key derivation
    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError>;

    /// Derive key using context for deterministic derivation
    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError>;

    // ====== Ed25519 Signatures ======

    /// Generate Ed25519 keypair
    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError>;

    /// Sign with Ed25519 private key
    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError>;

    /// Verify Ed25519 signature
    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError>;

    // ====== FROST Threshold Signatures ======

    /// Generate FROST threshold keypair shares
    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError>;

    /// Generate FROST signing nonces
    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError>;

    /// Create FROST signing package
    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
    ) -> Result<FrostSigningPackage, CryptoError>;

    /// Generate FROST signature share
    async fn frost_sign_share(
        &self,
        signing_package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError>;

    /// Aggregate FROST signature shares into final signature
    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError>;

    /// Verify FROST threshold signature
    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, CryptoError>;

    /// Extract public key from Ed25519 private key
    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError>;

    // ====== Symmetric Encryption ======

    /// Encrypt data with ChaCha20-Poly1305
    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError>;

    /// Decrypt data with ChaCha20-Poly1305
    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError>;

    /// Encrypt data with AES-GCM
    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError>;

    /// Decrypt data with AES-GCM
    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError>;

    // ====== Key Rotation & Resharing ======

    /// Rotate FROST threshold keys
    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError>;

    // ====== Utility Methods ======

    /// Check if this crypto handler supports simulation mode
    fn is_simulated(&self) -> bool;

    /// Get crypto implementation capabilities
    fn crypto_capabilities(&self) -> Vec<String>;

    /// Constant-time comparison for cryptographic values
    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool;

    /// Securely zero memory
    fn secure_zero(&self, data: &mut [u8]);
}
