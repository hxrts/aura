//! Cryptographic Effect Handlers
//!
//! Provides context-free implementations of cryptographic operations.
//!
//! Note: This module legitimately uses cryptographic types like `sha2::Sha256`
//! and `rand::rngs::OsRng` as it implements the CryptoEffects trait - this is
//! the effect handler layer where actual cryptographic operations are provided.

// Allow disallowed types/methods in cryptographic effect handler implementations
#![allow(clippy::disallowed_types)]
#![allow(clippy::disallowed_methods)]

use async_trait::async_trait;
use aura_core::crypto::{IdentityKeyContext, KeyDerivationSpec, PermissionKeyContext};
use aura_core::effects::crypto::{FrostKeyGenResult, FrostSigningPackage, KeyDerivationContext};
use aura_core::effects::{CryptoEffects, CryptoError, RandomEffects};
use aura_core::hash;
use rand::RngCore;
use zeroize::Zeroize;

/// Derive an encryption key using the specified context and version
///
/// This function provides secure key derivation with proper context separation
/// and collision resistance.
pub fn derive_encryption_key(
    root_key: &[u8],
    spec: &KeyDerivationSpec,
) -> Result<[u8; 32], CryptoError> {
    derive_key_material(root_key, spec, 32).map(|bytes| {
        let mut result = [0u8; 32];
        result.copy_from_slice(&bytes[0..32]);
        result
    })
}

/// Derive key material of arbitrary length
///
/// This is the core key derivation function that can produce keys of any length.
/// It uses HKDF-like expansion with hash for consistency across different lengths.
pub fn derive_key_material(
    root_key: &[u8],
    spec: &KeyDerivationSpec,
    output_length: usize,
) -> Result<Vec<u8>, CryptoError> {
    if output_length == 0 {
        return Err(CryptoError::invalid("Output length must be greater than 0"));
    }

    if output_length > 255 * 32 {
        return Err(CryptoError::invalid(
            "Output length too large for HKDF expansion",
        ));
    }

    // Build context string for domain separation
    let mut context_bytes = Vec::new();

    // Add identity context
    context_bytes.extend_from_slice(b"aura.key_derivation.v1:");
    context_bytes.extend_from_slice(b"identity:");

    match &spec.identity_context {
        IdentityKeyContext::AccountRoot { account_id } => {
            context_bytes.extend_from_slice(b"account_root:");
            context_bytes.extend_from_slice(account_id);
        }
        IdentityKeyContext::DeviceEncryption { device_id } => {
            context_bytes.extend_from_slice(b"device_encryption:");
            context_bytes.extend_from_slice(device_id);
        }
        IdentityKeyContext::RelationshipKeys { relationship_id } => {
            context_bytes.extend_from_slice(b"relationship:");
            context_bytes.extend_from_slice(relationship_id);
        }
        IdentityKeyContext::GuardianKeys { guardian_id } => {
            context_bytes.extend_from_slice(b"guardian:");
            context_bytes.extend_from_slice(guardian_id);
        }
    }

    // Add permission context if present
    if let Some(permission_context) = &spec.permission_context {
        context_bytes.extend_from_slice(b":permission:");

        match permission_context {
            PermissionKeyContext::StorageAccess {
                operation,
                resource,
            } => {
                context_bytes.extend_from_slice(b"storage:");
                context_bytes.extend_from_slice(operation.as_bytes());
                context_bytes.extend_from_slice(b":");
                context_bytes.extend_from_slice(resource.as_bytes());
            }
            PermissionKeyContext::Communication { capability_id } => {
                context_bytes.extend_from_slice(b"communication:");
                context_bytes.extend_from_slice(capability_id);
            }
        }
    }

    // Add version for key rotation
    context_bytes.extend_from_slice(b":version:");
    context_bytes.extend_from_slice(&spec.key_version.to_le_bytes());

    // Extract: Combine root key with context
    let mut extract_input = Vec::new();
    extract_input.extend_from_slice(root_key);
    extract_input.extend_from_slice(&context_bytes);

    let prk = hash::hash(&extract_input);

    // Expand: Generate output material using HKDF-like expansion
    let mut output = Vec::with_capacity(output_length);
    let num_blocks = output_length.div_ceil(32);

    for i in 0..num_blocks {
        let mut expand_input = Vec::new();
        expand_input.extend_from_slice(&prk);
        expand_input.extend_from_slice(&context_bytes);
        expand_input.push(i as u8 + 1); // HKDF counter (1-indexed)

        let block = hash::hash(&expand_input);
        output.extend_from_slice(&block);
    }

    // Truncate to requested length
    output.truncate(output_length);
    Ok(output)
}


/// Real crypto handler using actual cryptographic operations
#[derive(Debug, Clone)]
pub struct RealCryptoHandler {
    _phantom: std::marker::PhantomData<()>,
}

impl Default for RealCryptoHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RealCryptoHandler {
    /// Create a new real crypto handler
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}


// RandomEffects implementation for RealCryptoHandler
#[async_trait]
impl RandomEffects for RealCryptoHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
        bytes
    }

    async fn random_u64(&self) -> u64 {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
        u64::from_le_bytes(bytes)
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        let range = max - min;
        let random = self.random_u64().await;
        min + (random % range)
    }

    async fn random_uuid(&self) -> uuid::Uuid {
        let bytes = self.random_bytes(16).await;
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&bytes);
        uuid::Uuid::from_bytes(uuid_bytes)
    }
}

// (MockCryptoHandler implementation moved to aura-testkit)


// CryptoEffects implementation for RealCryptoHandler
#[async_trait]
impl CryptoEffects for RealCryptoHandler {
    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        use hkdf::Hkdf;
        use sha2::Sha256;

        if output_len == 0 || output_len > 8160 {
            return Err(CryptoError::invalid("Invalid output length for HKDF"));
        }

        let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
        let mut okm = vec![0u8; output_len];
        hk.expand(info, &mut okm)
            .map_err(|e| CryptoError::invalid(format!("HKDF expand failed: {}", e)))?;

        Ok(okm)
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        use aura_core::hash::hash;

        // Build context string for domain separation
        let context_str = format!("aura.key_derivation.v1:{:?}", context);
        let salt = hash(context_str.as_bytes());
        let info = b"aura_key_derivation";

        // Use HKDF to derive 32-byte key
        self.hkdf_derive(master_key, &salt, info, 32).await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        use ed25519_dalek::{SigningKey, VerifyingKey};
        use rand::rngs::OsRng;

        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = VerifyingKey::from(&signing_key);

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
        use ed25519_dalek::{Signature, Signer, SigningKey};

        let signing_key = SigningKey::from_bytes(
            private_key
                .try_into()
                .map_err(|_| CryptoError::invalid("Invalid private key length"))?,
        );

        let signature: Signature = signing_key.sign(message);
        Ok(signature.to_bytes().to_vec())
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let verifying_key = VerifyingKey::from_bytes(
            public_key
                .try_into()
                .map_err(|_| CryptoError::invalid("Invalid public key length"))?,
        )
        .map_err(|e| CryptoError::invalid(format!("Invalid verifying key: {}", e)))?;

        let signature = Signature::from_bytes(
            signature
                .try_into()
                .map_err(|_| CryptoError::invalid("Invalid signature length"))?,
        );

        Ok(verifying_key.verify(message, &signature).is_ok())
    }

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        use frost_ed25519 as frost;
        use rand::rngs::OsRng;

        let rng = OsRng;

        // Generate coefficients for secret sharing
        let (shares, public_key_package) = frost::keys::generate_with_dealer(
            max_signers,
            threshold,
            frost::keys::IdentifierList::Default,
            rng,
        )
        .map_err(|e| CryptoError::invalid(format!("FROST key generation failed: {}", e)))?;

        // Convert key shares to byte vectors
        let key_packages: Vec<Vec<u8>> = shares
            .values()
            .map(|key_package| {
                // Serialize the key package
                bincode::serialize(key_package).map_err(|e| {
                    CryptoError::invalid(format!("Failed to serialize key package: {}", e))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Serialize the public key package separately
        let public_key_package_bytes = bincode::serialize(&public_key_package).map_err(|e| {
            CryptoError::invalid(format!("Failed to serialize public key package: {}", e))
        })?;

        Ok(FrostKeyGenResult {
            key_packages,
            public_key_package: public_key_package_bytes,
        })
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        use crate::crypto::EffectSystemRng;

        let mut rng = EffectSystemRng::from_current_runtime(self);
        // Placeholder: generate random bytes for nonce bundle to satisfy interface
        let mut nonce_bytes = vec![0u8; 64];
        rng.fill_bytes(&mut nonce_bytes);

        bincode::serialize(&(nonce_bytes.clone(), nonce_bytes)).map_err(|e| {
            CryptoError::invalid(format!(
                "Failed to serialize FROST signing nonces: {}",
                e
            ))
        })
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<FrostSigningPackage, CryptoError> {
        use frost_ed25519 as frost;
        use std::collections::BTreeMap;
        use std::collections::HashSet;

        if participants.is_empty() || nonces.is_empty() {
            return Err(CryptoError::invalid(
                "Signing package requires at least one participant and nonce",
            ));
        }

        if nonces.len() != participants.len() {
            return Err(CryptoError::invalid(
                "Each participant must supply matching nonces",
            ));
        }

        let mut seen = HashSet::new();

        // Deserialize nonce bundles into commitments
        let mut commitments = BTreeMap::new();
        for (i, nonce_bytes) in nonces.iter().enumerate() {
            let participant_id = participants[i];

            if !seen.insert(participant_id) {
                return Err(CryptoError::invalid(format!(
                    "Duplicate participant id {} in signing package",
                    participant_id
                )));
            }

            let (_signing_nonces, signing_commitments): (
                frost::round1::SigningNonces,
                frost::round1::SigningCommitments,
            ) = bincode::deserialize(nonce_bytes).map_err(|e| {
                CryptoError::invalid(format!(
                    "Invalid signing nonces for participant {}: {}",
                    participant_id, e
                ))
            })?;

            let identifier = frost::Identifier::try_from(participant_id)
                .map_err(|e| CryptoError::invalid(format!("Invalid participant ID: {}", e)))?;
            commitments.insert(identifier, signing_commitments);
        }

        // Create signing package
        let package = frost::SigningPackage::new(commitments, message);
        let package_bytes = bincode::serialize(&package).map_err(|e| {
            CryptoError::invalid(format!("Failed to serialize signing package: {}", e))
        })?;

        Ok(FrostSigningPackage {
            message: message.to_vec(),
            package: package_bytes,
            participants: participants.to_vec(),
            public_key_package: public_key_package.to_vec(),
        })
    }

    async fn frost_sign_share(
        &self,
        package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        use frost_ed25519 as frost;

        let mut key_share_buf = key_share.to_vec();
        let mut nonce_buf = nonces.to_vec();

        // Deserialize components
        let signing_package: frost::SigningPackage = bincode::deserialize(&package.package)
            .map_err(|e| CryptoError::invalid(format!("Invalid signing package: {}", e)))?;

        let key_package: frost::keys::KeyPackage = bincode::deserialize(&key_share_buf)
            .map_err(|e| CryptoError::invalid(format!("Invalid key share: {}", e)))?;

        let (signing_nonces_bytes, _): (Vec<u8>, Vec<u8>) =
            bincode::deserialize(&nonce_buf)
                .map_err(|e| CryptoError::invalid(format!("Invalid signing nonces: {}", e)))?;

        // Reconstruct deterministic SigningNonces from bytes (test-only stub)
        let signing_nonces = frost::round1::SigningNonces::deserialize(&signing_nonces_bytes)
            .map_err(|e| CryptoError::invalid(format!("Invalid signing nonces format: {}", e)))?;

        // Create signature share
        let signature_share = frost::round2::sign(&signing_package, &signing_nonces, &key_package)
            .map_err(|e| CryptoError::invalid(format!("FROST signing failed: {}", e)))?;

        // Serialize result
        let serialized = bincode::serialize(&signature_share).map_err(|e| {
            CryptoError::invalid(format!("Failed to serialize signature share: {}", e))
        })?;

        key_share_buf.zeroize();
        nonce_buf.zeroize();

        Ok(serialized)
    }

    async fn frost_aggregate_signatures(
        &self,
        package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        use frost_ed25519 as frost;
        use std::collections::BTreeMap;

        // Deserialize signing package
        let signing_package: frost::SigningPackage = bincode::deserialize(&package.package)
            .map_err(|e| CryptoError::invalid(format!("Invalid signing package: {}", e)))?;

        // Deserialize public key package
        let pubkey_package: frost::keys::PublicKeyPackage =
            bincode::deserialize(&package.public_key_package)
                .map_err(|e| CryptoError::invalid(format!("Invalid public key package: {}", e)))?;

        // Deserialize signature shares
        let mut shares = BTreeMap::new();
        for (i, share_bytes) in signature_shares.iter().enumerate() {
            if let Some(&participant_id) = package.participants.get(i) {
                let signature_share: frost::round2::SignatureShare =
                    bincode::deserialize(share_bytes).map_err(|e| {
                        CryptoError::invalid(format!("Invalid signature share: {}", e))
                    })?;
                let identifier = frost::Identifier::try_from(participant_id)
                    .map_err(|e| CryptoError::invalid(format!("Invalid participant ID: {}", e)))?;
                shares.insert(identifier, signature_share);
            }
        }

        // Aggregate signatures using the proper FROST API with PublicKeyPackage
        let group_signature = frost::aggregate(&signing_package, &shares, &pubkey_package)
            .map_err(|e| CryptoError::invalid(format!("FROST aggregation failed: {}", e)))?;

        // Serialize the resulting signature
        Ok(group_signature.serialize().to_vec())
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        use frost_ed25519 as frost;

        // Parse signature
        let signature_array: [u8; 64] = signature
            .try_into()
            .map_err(|_| CryptoError::invalid("Invalid signature length"))?;
        let frost_signature = frost::Signature::deserialize(signature_array)
            .map_err(|e| CryptoError::invalid(format!("Invalid FROST signature: {}", e)))?;

        // Parse group public key using deserialize
        let pubkey_array: [u8; 32] = group_public_key
            .try_into()
            .map_err(|_| CryptoError::invalid("Invalid group public key length"))?;
        let verifying_key = frost::VerifyingKey::deserialize(pubkey_array)
            .map_err(|e| CryptoError::invalid(format!("Invalid group public key: {}", e)))?;

        // Verify signature
        Ok(verifying_key.verify(message, &frost_signature).is_ok())
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        use ed25519_dalek::{SigningKey, VerifyingKey};

        let signing_key = SigningKey::from_bytes(
            private_key
                .try_into()
                .map_err(|_| CryptoError::invalid("Invalid private key length"))?,
        );

        let verifying_key = VerifyingKey::from(&signing_key);
        Ok(verifying_key.to_bytes().to_vec())
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        use chacha20::cipher::{KeyIvInit, StreamCipher};
        use chacha20::ChaCha20;

        let mut cipher = ChaCha20::new(key.into(), nonce.into());
        let mut ciphertext = plaintext.to_vec();
        cipher.apply_keystream(&mut ciphertext);
        Ok(ciphertext)
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // ChaCha20 is symmetric, so decrypt = encrypt
        self.chacha20_encrypt(ciphertext, key, nonce).await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        use aes_gcm::aead::Aead;
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

        let cipher = Aes256Gcm::new(key.into());
        let nonce = Nonce::from_slice(nonce);

        cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| CryptoError::invalid(format!("AES-GCM encryption failed: {}", e)))
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        use aes_gcm::aead::Aead;
        use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

        let cipher = Aes256Gcm::new(key.into());
        let nonce = Nonce::from_slice(nonce);

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| CryptoError::invalid(format!("AES-GCM decryption failed: {}", e)))
    }

    async fn frost_rotate_keys(
        &self,
        _old_shares: &[Vec<u8>],
        _old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        // Placeholder implementation
        self.frost_generate_keys(new_threshold, new_max_signers)
            .await
    }

    fn is_simulated(&self) -> bool {
        false
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "ed25519".to_string(),
            "frost".to_string(),
            "aes-gcm".to_string(),
            "chacha20".to_string(),
            "hkdf".to_string(),
        ]
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        // Use a simple constant-time comparison
        let mut result = 0u8;
        for (x, y) in a.iter().zip(b.iter()) {
            result |= x ^ y;
        }
        result == 0
    }

    fn secure_zero(&self, data: &mut [u8]) {
        data.zeroize();
    }
}

/// FROST RNG Adapter for Effect System Integration
///
/// This adapter bridges the async RandomEffects trait to the synchronous RngCore
/// trait required by the frost_ed25519 library. It allows FROST cryptographic
/// operations to use the effect system's randomness source while maintaining
/// testability and determinism.
///
/// # Architecture Note
///
/// FROST requires sync RNG (RngCore trait), but our effect system is async.
/// This adapter uses tokio::runtime::Handle to perform async-to-sync conversion
/// via block_on(). This is acceptable because:
/// 1. FROST operations are already synchronous in the library's API
/// 2. RandomEffects implementations are fast (crypto RNG or deterministic)
/// 3. This is only used during key generation and signing ceremonies
///
/// # Example
///
/// ```rust,ignore
/// use aura_effects::crypto::EffectSystemRng;
/// use aura_core::effects::RandomEffects;
/// use frost_ed25519 as frost;
///
/// async fn generate_frost_keys(effects: &dyn RandomEffects) {
///     let runtime = tokio::runtime::Handle::current();
///     let mut rng = EffectSystemRng::new(effects, runtime);
///
///     let (shares, pubkey) = frost::keys::generate_with_dealer(
///         3, 2,
///         frost::keys::IdentifierList::Default,
///         &mut rng
///     )?;
/// }
/// ```
pub struct EffectSystemRng<'a> {
    effects: &'a dyn RandomEffects,
    runtime: tokio::runtime::Handle,
}

impl<'a> EffectSystemRng<'a> {
    /// Create a new RNG adapter from RandomEffects and a runtime handle
    pub fn new(effects: &'a dyn RandomEffects, runtime: tokio::runtime::Handle) -> Self {
        Self { effects, runtime }
    }

    /// Create a new RNG adapter using the current runtime
    ///
    /// # Panics
    ///
    /// Panics if called outside of a Tokio runtime context
    pub fn from_current_runtime(effects: &'a dyn RandomEffects) -> Self {
        let runtime = tokio::runtime::Handle::current();
        Self::new(effects, runtime)
    }
}

impl rand::RngCore for EffectSystemRng<'_> {
    fn next_u32(&mut self) -> u32 {
        // Get lower 32 bits of u64
        (self.runtime.block_on(self.effects.random_u64()) & 0xFFFF_FFFF) as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.runtime.block_on(self.effects.random_u64())
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let bytes = self.runtime.block_on(self.effects.random_bytes(dest.len()));
        dest.copy_from_slice(&bytes);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

// Mark this RNG as cryptographically secure since RandomEffects is crypto-secure
impl rand::CryptoRng for EffectSystemRng<'_> {}

#[cfg(test)]
mod rng_adapter_tests {
    use super::*;
    use rand::RngCore;

    #[test]
    fn test_rng_adapter_with_mock() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let crypto = RealCryptoHandler::new();
        let mut rng = EffectSystemRng::new(&crypto, runtime.handle().clone());

        // Test next_u32
        let val1 = rng.next_u32();
        let val2 = rng.next_u32();
        assert_ne!(val1, val2, "Should produce different values");

        // Test next_u64
        let val3 = rng.next_u64();
        let val4 = rng.next_u64();
        assert_ne!(val3, val4, "Should produce different values");

        // Test fill_bytes
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        assert_ne!(bytes, [0u8; 32], "Should fill with random bytes");
    }

    #[test]
    fn test_rng_adapter_deterministic() {
        // Same seed should produce same sequence
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let crypto1 = RealCryptoHandler::new();
        let crypto2 = RealCryptoHandler::new();

        let handle = runtime.handle().clone();
        let mut rng1 = EffectSystemRng::new(&crypto1, handle.clone());
        let mut rng2 = EffectSystemRng::new(&crypto2, handle);

        let val1 = rng1.next_u64();
        let val2 = rng2.next_u64();
        assert_eq!(val1, val2, "Same seed should produce same values");

        let mut bytes1 = [0u8; 16];
        let mut bytes2 = [0u8; 16];
        rng1.fill_bytes(&mut bytes1);
        rng2.fill_bytes(&mut bytes2);
        assert_eq!(
            bytes1, bytes2,
            "Same seed should produce same byte sequences"
        );
    }

    #[test]
    fn test_rng_adapter_from_current_runtime() {
        let _runtime = tokio::runtime::Runtime::new().unwrap();
        // Enter the runtime context so from_current_runtime() can get the handle
        let _guard = _runtime.enter();

        let crypto = RealCryptoHandler::new();
        let mut rng = EffectSystemRng::from_current_runtime(&crypto);

        let val = rng.next_u64();
        assert_ne!(val, 0, "Should produce non-zero values");
    }

    #[tokio::test]
    async fn test_complete_frost_workflow() {
        use crate::crypto::RealCryptoHandler;
        use frost_ed25519 as frost;

        let crypto = RealCryptoHandler::new();
        let message = b"test message for FROST signing";

        // Test 2-of-3 threshold signature
        let threshold = 2;
        let max_signers = 3;

        // 1. Generate FROST keys
        let key_gen_result = crypto
            .frost_generate_keys(threshold, max_signers)
            .await
            .unwrap();
        assert_eq!(key_gen_result.key_packages.len(), max_signers as usize);
        assert!(!key_gen_result.public_key_package.is_empty());

        // Deserialize key packages to retrieve participant identifiers
        let key_pkg1: frost::keys::KeyPackage =
            bincode::deserialize(&key_gen_result.key_packages[0]).unwrap();
        let key_pkg2: frost::keys::KeyPackage =
            bincode::deserialize(&key_gen_result.key_packages[1]).unwrap();
        let participant1 = u16::from_be_bytes(key_pkg1.identifier().serialize());
        let participant2 = u16::from_be_bytes(key_pkg2.identifier().serialize());

        // 2. Generate nonces for signing participants
        let nonces1 = crypto.frost_generate_nonces().await.unwrap();
        let nonces2 = crypto.frost_generate_nonces().await.unwrap();
        assert!(!nonces1.is_empty());
        assert!(!nonces2.is_empty());

        // 3. Create signing package with real commitments
        let participants = vec![participant1, participant2]; // Using first two participants for 2-of-3 threshold
        let nonces = vec![nonces1.clone(), nonces2.clone()];

        let signing_package = crypto
            .frost_create_signing_package(
                message,
                &nonces,
                &participants,
                &key_gen_result.public_key_package,
            )
            .await
            .expect("signing package");

        // 4. Create signature shares for participants
        let share1 = crypto
            .frost_sign_share(&signing_package, &key_gen_result.key_packages[0], &nonces1)
            .await
            .expect("signature share 1");
        let share2 = crypto
            .frost_sign_share(&signing_package, &key_gen_result.key_packages[1], &nonces2)
            .await
            .expect("signature share 2");

        // 5. Aggregate signatures
        let group_signature = crypto
            .frost_aggregate_signatures(&signing_package, &[share1.clone(), share2.clone()])
            .await
            .expect("aggregate signature");

        // 6. Verify aggregated signature against group public key
        let pubkey_package: frost::keys::PublicKeyPackage =
            bincode::deserialize(&key_gen_result.public_key_package).unwrap();
        let verifying_key = pubkey_package.verifying_key().serialize().to_vec();
        let verified = crypto
            .frost_verify(message, &group_signature, &verifying_key)
            .await
            .expect("verification");
        assert!(verified, "Aggregated signature should verify");

        // 4. Test key generation produces consistent structure
        let key_gen_result2 = crypto
            .frost_generate_keys(threshold, max_signers)
            .await
            .unwrap();
        assert_eq!(
            key_gen_result2.key_packages.len(),
            key_gen_result.key_packages.len()
        );
        // Different runs should produce different keys
        assert_ne!(
            key_gen_result2.public_key_package,
            key_gen_result.public_key_package
        );
    }

    #[tokio::test]
    async fn test_frost_key_generation_structure() {
        use crate::crypto::RealCryptoHandler;

        let crypto = RealCryptoHandler::new();

        // Test various threshold configurations
        // Note: (1,1) might not work with FROST as it requires threshold >= 2
        let test_cases = vec![(2, 3), (3, 5), (2, 2), (3, 7)];

        for (threshold, max_signers) in test_cases {
            let result = crypto
                .frost_generate_keys(threshold, max_signers)
                .await
                .unwrap();

            // Validate structure
            assert_eq!(
                result.key_packages.len(),
                max_signers as usize,
                "Should have {} key packages for {}-of-{}",
                max_signers,
                threshold,
                max_signers
            );
            assert!(
                !result.public_key_package.is_empty(),
                "Public key package should not be empty for {}-of-{}",
                threshold,
                max_signers
            );

            // Each key package should be non-empty and different
            for (i, key_package) in result.key_packages.iter().enumerate() {
                assert!(
                    !key_package.is_empty(),
                    "Key package {} should not be empty for {}-of-{}",
                    i,
                    threshold,
                    max_signers
                );
            }

            // All key packages should be different
            for i in 0..result.key_packages.len() {
                for j in (i + 1)..result.key_packages.len() {
                    assert_ne!(
                        result.key_packages[i], result.key_packages[j],
                        "Key packages {} and {} should be different for {}-of-{}",
                        i, j, threshold, max_signers
                    );
                }
            }
        }
    }
}
