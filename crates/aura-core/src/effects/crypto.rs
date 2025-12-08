//! Cryptographic effects trait definitions
//!
//! This module defines the trait interfaces for cryptographic operations.
//! Actual cryptographic implementations are provided by aura-crypto crate.
//! Effect handlers that integrate aura-crypto are provided by aura-protocol handlers.
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Usage**: All crates needing cryptographic operations (signing, hashing, key derivation)
//!
//! This is an infrastructure effect that must be implemented in `aura-effects`
//! with stateless handlers. Domain crates should not implement this trait directly
//! but rather use it via dependency injection.

use super::RandomEffects;
use crate::types::identifiers::DeviceId;
use crate::{AccountId, AuraError};
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

/// FROST key generation result containing both individual key packages and the group public key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostKeyGenResult {
    /// Individual key packages for each participant
    pub key_packages: Vec<Vec<u8>>,
    /// Group public key package needed for signature aggregation and verification
    pub public_key_package: Vec<u8>,
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
    /// Public key package needed for aggregation
    pub public_key_package: Vec<u8>,
}

// Re-export SigningMode from crypto module for convenience
pub use crate::crypto::single_signer::SigningMode;

/// Result of signing key generation (unified for single-signer and threshold).
///
/// This type is returned by `generate_signing_keys()` and contains everything
/// needed to store and use the generated keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningKeyGenResult {
    /// Key packages (one per participant).
    ///
    /// For single-signer: contains one `SingleSignerKeyPackage` serialized.
    /// For threshold: contains FROST `KeyPackage` for each participant.
    pub key_packages: Vec<Vec<u8>>,

    /// Public key package for verification.
    ///
    /// For single-signer: contains `SingleSignerPublicKeyPackage` serialized.
    /// For threshold: contains FROST `PublicKeyPackage` serialized.
    pub public_key_package: Vec<u8>,

    /// Which signing mode was used.
    ///
    /// This determines which signing/verification algorithm to use.
    pub mode: SigningMode,
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
///
/// # Stability: STABLE
/// This is a core stable API with semver guarantees. Breaking changes require major version bump.
#[async_trait]
pub trait CryptoEffects: RandomEffects + Send + Sync {
    // Note: Hashing is NOT an algebraic effect - it's a pure operation.
    // Use aura_core::hash::hash() for synchronous hashing instead.
    // See docs/002_system_architecture.md for design rationale.

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

    // ====== Unified Signing Key Generation ======

    /// Generate signing keys for the given threshold configuration.
    ///
    /// This is the unified entry point for key generation that automatically
    /// selects the appropriate algorithm:
    /// - For `threshold=1, max_signers=1`: Generates Ed25519 single-signer keys
    /// - For `threshold>=2`: Generates FROST threshold keys via DKG
    ///
    /// # Arguments
    ///
    /// * `threshold` - Minimum signers required (1 for single-signer, >=2 for FROST)
    /// * `max_signers` - Total number of key shares to generate
    ///
    /// # Returns
    ///
    /// `SigningKeyGenResult` containing key packages and the signing mode used.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - `threshold > max_signers`
    /// - `threshold == 0`
    /// - Key generation fails
    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError>;

    /// Sign a message using the appropriate algorithm for the key type.
    ///
    /// # Arguments
    ///
    /// * `message` - The message bytes to sign
    /// * `key_package` - Serialized key package (SingleSignerKeyPackage or FROST KeyPackage)
    /// * `mode` - Which signing algorithm to use
    ///
    /// # Returns
    ///
    /// The 64-byte Ed25519 signature.
    ///
    /// # Errors
    ///
    /// - For `SingleSigner`: Returns error if key is invalid
    /// - For `Threshold`: Returns error; use full FROST protocol flow instead
    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError>;

    /// Verify a signature using the appropriate algorithm for the key type.
    ///
    /// # Arguments
    ///
    /// * `message` - The original message bytes
    /// * `signature` - The 64-byte signature to verify
    /// * `public_key_package` - Serialized public key package
    /// * `mode` - Which verification algorithm to use
    ///
    /// # Returns
    ///
    /// `true` if signature is valid, `false` otherwise.
    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, CryptoError>;

    // ====== FROST Threshold Signatures ======

    /// Generate FROST threshold keypair shares.
    ///
    /// **Note**: FROST requires `threshold >= 2`. For 1-of-1 configurations,
    /// use `generate_signing_keys(1, 1)` instead, which will use Ed25519.
    ///
    /// Prefer using `generate_signing_keys()` for new code as it handles
    /// both single-signer and threshold cases automatically.
    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError>;

    /// Generate FROST signing nonces for a participant
    ///
    /// The nonces are generated from the participant's key package signing share,
    /// ensuring they are valid for use in the threshold signing protocol.
    ///
    /// # Arguments
    /// * `key_package` - The participant's serialized FROST key package
    ///
    /// # Returns
    /// Serialized nonces and commitments bundle
    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError>;

    /// Create FROST signing package
    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
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
    ) -> Result<FrostKeyGenResult, CryptoError>;

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

/// Blanket implementation for Arc<T> where T: CryptoEffects
/// Note: CryptoEffects inherits RandomEffects, and Arc<T> gets RandomEffects from the blanket impl in random.rs
#[async_trait]
impl<T: CryptoEffects + ?Sized> CryptoEffects for std::sync::Arc<T> {
    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        (**self).hkdf_derive(ikm, salt, info, output_len).await
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        (**self).derive_key(master_key, context).await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        (**self).ed25519_generate_keypair().await
    }

    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        (**self).ed25519_sign(message, private_key).await
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        (**self)
            .ed25519_verify(message, signature, public_key)
            .await
    }

    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        (**self).generate_signing_keys(threshold, max_signers).await
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        (**self).sign_with_key(message, key_package, mode).await
    }

    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        (**self)
            .verify_signature(message, signature, public_key_package, mode)
            .await
    }

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        (**self).frost_generate_keys(threshold, max_signers).await
    }

    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        (**self).frost_generate_nonces(key_package).await
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<FrostSigningPackage, CryptoError> {
        (**self)
            .frost_create_signing_package(message, nonces, participants, public_key_package)
            .await
    }

    async fn frost_sign_share(
        &self,
        signing_package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        (**self)
            .frost_sign_share(signing_package, key_share, nonces)
            .await
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        (**self)
            .frost_aggregate_signatures(signing_package, signature_shares)
            .await
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        (**self)
            .frost_verify(message, signature, group_public_key)
            .await
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        (**self).ed25519_public_key(private_key).await
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        (**self).chacha20_encrypt(plaintext, key, nonce).await
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        (**self).chacha20_decrypt(ciphertext, key, nonce).await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        (**self).aes_gcm_encrypt(plaintext, key, nonce).await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        (**self).aes_gcm_decrypt(ciphertext, key, nonce).await
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        (**self)
            .frost_rotate_keys(old_shares, old_threshold, new_threshold, new_max_signers)
            .await
    }

    fn is_simulated(&self) -> bool {
        (**self).is_simulated()
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        (**self).crypto_capabilities()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        (**self).constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        (**self).secure_zero(data)
    }
}
