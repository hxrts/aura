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

use crate::effects::random::RandomCoreEffects;
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

/// Core cryptographic effects interface
///
/// This trait defines cryptographic operations for the Aura effects system.
/// The actual cryptographic primitives are implemented in aura-crypto crate.
/// Effect handlers are implemented in aura-protocol handlers for different environments:
/// - Production: Real cryptographic operations using aura-crypto
/// - Testing: Deterministic mock operations
/// - Simulation: Controlled cryptographic scenarios
///
/// CryptoCoreEffects inherits from RandomCoreEffects to ensure all randomness is deterministic
/// and controllable for simulation purposes.
///
/// # Stability: STABLE
/// This is a core stable API with semver guarantees. Breaking changes require major version bump.
#[async_trait]
pub trait CryptoCoreEffects: RandomCoreEffects + Send + Sync {
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

    /// Check if this crypto handler supports simulation mode
    fn is_simulated(&self) -> bool;

    /// Get crypto implementation capabilities
    fn crypto_capabilities(&self) -> Vec<String>;

    /// Constant-time comparison for cryptographic values
    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool;

    /// Securely zero memory
    fn secure_zero(&self, data: &mut [u8]);
}

/// Optional cryptographic effects that build on the core interface.
#[async_trait]
pub trait CryptoExtendedEffects: CryptoCoreEffects + Send + Sync {
    // ====== Unified Signing Key Generation ======

    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        let _ = (threshold, max_signers);
        Err(AuraError::crypto("generate_signing_keys not supported"))
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        let _ = (message, key_package, mode);
        Err(AuraError::crypto("sign_with_key not supported"))
    }

    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        let _ = (message, signature, public_key_package, mode);
        Err(AuraError::crypto("verify_signature not supported"))
    }

    // ====== FROST Threshold Signatures ======

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        let _ = (threshold, max_signers);
        Err(AuraError::crypto("frost_generate_keys not supported"))
    }

    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let _ = key_package;
        Err(AuraError::crypto("frost_generate_nonces not supported"))
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<FrostSigningPackage, CryptoError> {
        let _ = (message, nonces, participants, public_key_package);
        Err(AuraError::crypto(
            "frost_create_signing_package not supported",
        ))
    }

    async fn frost_sign_share(
        &self,
        signing_package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let _ = (signing_package, key_share, nonces);
        Err(AuraError::crypto("frost_sign_share not supported"))
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        let _ = (signing_package, signature_shares);
        Err(AuraError::crypto(
            "frost_aggregate_signatures not supported",
        ))
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        let _ = (message, signature, group_public_key);
        Err(AuraError::crypto("frost_verify not supported"))
    }

    /// Extract public key from Ed25519 private key
    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let _ = private_key;
        Err(AuraError::crypto("ed25519_public_key not supported"))
    }

    // ====== Symmetric Encryption ======

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        let _ = (plaintext, key, nonce);
        Err(AuraError::crypto("chacha20_encrypt not supported"))
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        let _ = (ciphertext, key, nonce);
        Err(AuraError::crypto("chacha20_decrypt not supported"))
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        let _ = (plaintext, key, nonce);
        Err(AuraError::crypto("aes_gcm_encrypt not supported"))
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        let _ = (ciphertext, key, nonce);
        Err(AuraError::crypto("aes_gcm_decrypt not supported"))
    }

    // ====== Key Rotation & Resharing ======

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        let _ = (old_shares, old_threshold, new_threshold, new_max_signers);
        Err(AuraError::crypto("frost_rotate_keys not supported"))
    }
}

/// Combined cryptographic effects surface (core + extended).
pub trait CryptoEffects: CryptoCoreEffects + CryptoExtendedEffects {}

impl<T: CryptoCoreEffects + CryptoExtendedEffects + ?Sized> CryptoEffects for T {}

/// Blanket implementation for Arc<T> where T: CryptoCoreEffects
/// Note: CryptoCoreEffects inherits RandomCoreEffects, and Arc<T> gets RandomCoreEffects from the blanket impl in random.rs
#[async_trait]
impl<T: CryptoCoreEffects + ?Sized> CryptoCoreEffects for std::sync::Arc<T> {
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
