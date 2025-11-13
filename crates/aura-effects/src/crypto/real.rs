//! Real cryptographic handler using actual crypto operations
//!
//! Provides secure cryptographic operations for production use.
//!
//! This module is the implementation layer that wraps low-level cryptographic
//! and random number generation operations. It's expected to use disallowed methods
//! like `rand::thread_rng()` since its purpose is to abstract these operations
//! for the effect system.
#![allow(clippy::disallowed_methods)]

use async_trait::async_trait;
use aura_core::effects::crypto::{FrostSigningPackage, KeyDerivationContext};
use aura_core::effects::{CryptoEffects, CryptoError, RandomEffects};
// NOTE: aura-frost is a feature-level crate, not a dependency of basic handlers
// If FROST coordination is needed, it should be in aura-protocol, not here
// use aura_frost::{DkgCoordinator, FrostSigningCoordinator};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

/// Real cryptographic handler for production use
pub struct RealCryptoHandler {
    // Note: For thread safety, we use thread_rng() directly in methods rather than storing RNG
}

impl RealCryptoHandler {
    /// Create a new real crypto handler
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for RealCryptoHandler {
    fn default() -> Self {
        Self::new()
    }
}

// First implement RandomEffects
#[async_trait]
impl RandomEffects for RealCryptoHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        // Note: This is not thread-safe, but for simplicity we'll use thread_rng
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }

    async fn random_u64(&self) -> u64 {
        rand::thread_rng().next_u64()
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        use rand::Rng;
        rand::thread_rng().gen_range(min..max)
    }
}

// Then implement CryptoEffects (which inherits from RandomEffects)
#[async_trait]
impl CryptoEffects for RealCryptoHandler {
    // Note: Hashing is NOT an effect - use aura_core::hash::hash() instead

    // ====== Key Derivation ======

    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        let hkdf = Hkdf::<Sha256>::new(Some(salt), ikm);
        let mut output = vec![0u8; output_len];

        hkdf.expand(info, &mut output)
            .map_err(|e| aura_core::AuraError::crypto(format!("HKDF expansion failed: {}", e)))?;

        Ok(output)
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        let info = format!(
            "{}:{}:{:?}:{}:{}",
            context.app_id,
            context.context,
            context.derivation_path,
            context.account_id,
            context.device_id
        );
        self.hkdf_derive(master_key, b"", info.as_bytes(), 32).await
    }

    // ====== Ed25519 Signatures ======

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();
        Ok((
            signing_key.to_bytes().to_vec(),
            verifying_key.to_bytes().to_vec(),
        ))
    }

    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        if private_key.len() != 32 {
            return Err(aura_core::AuraError::invalid(
                "Ed25519 private key must be 32 bytes",
            ));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(private_key);
        let signing_key = SigningKey::from_bytes(&key_bytes);
        use ed25519_dalek::Signer;
        let signature = signing_key.sign(message);
        Ok(signature.to_bytes().to_vec())
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        if signature.len() != 64 {
            return Err(aura_core::AuraError::invalid(
                "Ed25519 signature must be 64 bytes",
            ));
        }
        if public_key.len() != 32 {
            return Err(aura_core::AuraError::invalid(
                "Ed25519 public key must be 32 bytes",
            ));
        }

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(signature);
        let signature = Signature::from_bytes(&sig_bytes);

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(public_key);
        let verifying_key = VerifyingKey::from_bytes(&key_bytes).map_err(|e| {
            aura_core::AuraError::invalid(format!("Invalid Ed25519 public key: {}", e))
        })?;

        match verifying_key.verify_strict(message, &signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if private_key.len() != 32 {
            return Err(aura_core::AuraError::invalid(
                "Ed25519 private key must be 32 bytes",
            ));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(private_key);
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();
        Ok(verifying_key.to_bytes().to_vec())
    }

    // ====== FROST Threshold Signatures ======
    //
    // NOTE: FROST is a complex multi-party protocol requiring coordination between
    // participants. These basic effect handlers are stubs. For production use:
    //   - Use aura-frost crate for complete FROST implementation
    //   - Use aura-protocol choreographies for multi-party coordination
    //   - Use aura-crypto::frost for tree-based signing helpers

    async fn frost_generate_keys(
        &self,
        _threshold: u16,
        _max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        Err(aura_core::AuraError::internal(
            "FROST key generation requires multi-party coordination. Use aura-frost or aura-protocol."
        ))
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        Err(aura_core::AuraError::internal(
            "FROST nonce generation requires coordination. Use aura-frost or aura-protocol.",
        ))
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
    ) -> Result<FrostSigningPackage, CryptoError> {
        // This is just a data structure wrapper, can be implemented
        Ok(FrostSigningPackage {
            message: message.to_vec(),
            participants: participants.to_vec(),
            package: nonces.concat(),
        })
    }

    async fn frost_sign_share(
        &self,
        _signing_package: &FrostSigningPackage,
        _key_share: &[u8],
        _nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(aura_core::AuraError::internal(
            "FROST signature share creation requires coordination. Use aura-frost or aura-protocol."
        ))
    }

    async fn frost_aggregate_signatures(
        &self,
        _signing_package: &FrostSigningPackage,
        _signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(aura_core::AuraError::internal(
            "FROST signature aggregation requires coordination. Use aura-frost or aura-protocol.",
        ))
    }

    async fn frost_verify(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        Err(aura_core::AuraError::internal(
            "FROST verification should use the frost-ed25519 library directly or aura-crypto helpers."
        ))
    }

    // ====== Symmetric Encryption ======

    async fn chacha20_encrypt(
        &self,
        _plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(aura_core::AuraError::internal(
            "Not implemented: ChaCha20 encryption not implemented yet".to_string(),
        ))
    }

    async fn chacha20_decrypt(
        &self,
        _ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(aura_core::AuraError::internal(
            "Not implemented: ChaCha20 decryption not implemented yet".to_string(),
        ))
    }

    async fn aes_gcm_encrypt(
        &self,
        _plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(aura_core::AuraError::internal(
            "Not implemented: AES-GCM encryption not implemented yet".to_string(),
        ))
    }

    async fn aes_gcm_decrypt(
        &self,
        _ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(aura_core::AuraError::internal(
            "Not implemented: AES-GCM decryption not implemented yet".to_string(),
        ))
    }

    // ====== Key Rotation & Resharing ======

    async fn frost_rotate_keys(
        &self,
        _old_shares: &[Vec<u8>],
        _old_threshold: u16,
        _new_threshold: u16,
        _new_max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        // FROST key rotation/resharing is a complex distributed protocol
        // that requires coordination between all participants.
        Err(aura_core::AuraError::internal(
            "FROST key rotation requires multi-party coordination. Use aura-frost or aura-protocol."
        ))
    }

    // ====== Utility Methods ======

    fn is_simulated(&self) -> bool {
        false
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "blake3_hash".to_string(),
            "sha256_hash".to_string(),
            "blake3_hmac".to_string(),
            "hkdf_derive".to_string(),
            "derive_key".to_string(),
            "ed25519_generate_keypair".to_string(),
            "ed25519_sign".to_string(),
            "ed25519_verify".to_string(),
            "ed25519_public_key".to_string(),
            "constant_time_eq".to_string(),
            "secure_zero".to_string(),
        ]
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        if a.len() != b.len() {
            return false;
        }
        a.ct_eq(b).into()
    }

    fn secure_zero(&self, data: &mut [u8]) {
        use zeroize::Zeroize;
        data.zeroize();
    }
}
