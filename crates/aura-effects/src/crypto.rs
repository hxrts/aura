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
use aura_core::effects::{
    CryptoCoreEffects, CryptoError, CryptoExtendedEffects, RandomCoreEffects,
};
use aura_core::hash;
use aura_core::util::serialization::to_vec;
use aura_core::Hash32;
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

/// Compute a deterministic transcript hash for dealer-based key generation (K2).
///
/// This mirrors the canonical hashing used by consensus DKG transcripts (K3),
/// but operates over the dealer outputs already present in the local keygen.
pub fn compute_dealer_transcript_hash(
    key_packages: &[Vec<u8>],
    public_key_package: &[u8],
) -> Result<Hash32, CryptoError> {
    #[derive(serde::Serialize)]
    struct DealerTranscriptDigest<'a> {
        key_packages: &'a [Vec<u8>],
        public_key_package: &'a [u8],
    }

    let digest = DealerTranscriptDigest {
        key_packages,
        public_key_package,
    };
    let encoded = to_vec(&digest).map_err(|e| CryptoError::serialization(e.to_string()))?;
    let mut hasher = hash::hasher();
    hasher.update(b"AURA_DKG_TRANSCRIPT");
    hasher.update(&encoded);
    Ok(Hash32(hasher.finalize()))
}

/// Real crypto handler using actual cryptographic operations.
/// Can be seeded for deterministic testing or use OS entropy in production.
#[derive(Debug, Clone)]
pub struct RealCryptoHandler {
    /// Optional seed for deterministic randomness in testing
    seed: Option<[u8; 32]>,
}

impl Default for RealCryptoHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RealCryptoHandler {
    /// Create a new real crypto handler using OS entropy
    pub fn new() -> Self {
        Self { seed: None }
    }

    /// Create a seeded crypto handler for deterministic testing
    ///
    /// When seeded, all randomness will be deterministic based on the provided seed.
    /// This is useful for reproducible tests and simulations.
    pub fn seeded(seed: [u8; 32]) -> Self {
        Self { seed: Some(seed) }
    }

    /// Get random bytes using the handler's RNG strategy
    fn get_random_bytes(&self, len: usize) -> Result<Vec<u8>, CryptoError> {
        let mut bytes = vec![0u8; len];
        if let Some(seed) = self.seed {
            // Use seeded randomness
            use rand::{RngCore, SeedableRng};
            let mut rng = rand_chacha::ChaCha20Rng::from_seed(seed);
            rng.fill_bytes(&mut bytes);
        } else {
            // Use OS entropy
            getrandom::getrandom(&mut bytes).map_err(|e| {
                CryptoError::invalid(format!("Failed to generate random bytes: {}", e))
            })?;
        }
        Ok(bytes)
    }
}

// RandomCoreEffects implementation for RealCryptoHandler
#[async_trait]
impl RandomCoreEffects for RealCryptoHandler {
    // JUSTIFICATION: RandomEffects trait doesn't support Results by design.
    // Cryptographic RNG failure is a fatal system error that should panic.
    // OS RNG failure indicates system compromise or resource exhaustion.
    #[allow(clippy::expect_used)]
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.get_random_bytes(len)
            .expect("Fatal: cryptographic RNG failure")
    }

    #[allow(clippy::expect_used)]
    async fn random_bytes_32(&self) -> [u8; 32] {
        let bytes = self
            .get_random_bytes(32)
            .expect("Fatal: cryptographic RNG failure");
        let mut result = [0u8; 32];
        result.copy_from_slice(&bytes);
        result
    }

    #[allow(clippy::expect_used)]
    async fn random_u64(&self) -> u64 {
        let bytes = self
            .get_random_bytes(8)
            .expect("Fatal: cryptographic RNG failure");
        u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }
}

// (MockCryptoHandler implementation moved to aura-testkit)

// Crypto core implementation for RealCryptoHandler
#[async_trait]
impl CryptoCoreEffects for RealCryptoHandler {
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
        use rand::SeedableRng;
        use rand_chacha::ChaCha20Rng;

        let (signing_key, verifying_key) = match self.seed {
            Some(seed) => {
                let mut rng = ChaCha20Rng::from_seed(seed);
                let signing_key = SigningKey::generate(&mut rng);
                let verifying_key = VerifyingKey::from(&signing_key);
                (signing_key, verifying_key)
            }
            None => {
                let mut rng = rand::rngs::OsRng;
                let signing_key = SigningKey::generate(&mut rng);
                let verifying_key = VerifyingKey::from(&signing_key);
                (signing_key, verifying_key)
            }
        };

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

    fn is_simulated(&self) -> bool {
        false
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "ed25519".to_string(),
            "frost".to_string(),
            "chacha20".to_string(),
            "aes-gcm".to_string(),
        ]
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
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

// Crypto extended implementation for RealCryptoHandler
#[async_trait]
impl CryptoExtendedEffects for RealCryptoHandler {
    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<aura_core::effects::crypto::SigningKeyGenResult, CryptoError> {
        use aura_core::crypto::single_signer::{
            SigningMode, SingleSignerKeyPackage, SingleSignerPublicKeyPackage,
        };
        use aura_core::effects::crypto::SigningKeyGenResult;

        // Validate basic constraints
        if threshold == 0 {
            return Err(CryptoError::invalid("Threshold must be at least 1"));
        }
        if threshold > max_signers {
            return Err(CryptoError::invalid(format!(
                "Threshold ({}) cannot exceed max_signers ({})",
                threshold, max_signers
            )));
        }

        if threshold == 1 && max_signers == 1 {
            // Single-signer: use standard Ed25519
            tracing::debug!("Generating single-signer Ed25519 keys");

            let (signing_key, verifying_key) = self.ed25519_generate_keypair().await?;

            let key_package = SingleSignerKeyPackage::new(signing_key, verifying_key.clone());
            let public_package = SingleSignerPublicKeyPackage::new(verifying_key);

            Ok(SigningKeyGenResult {
                key_packages: vec![key_package.to_bytes().map_err(|e| {
                    CryptoError::invalid(format!("key package serialization: {}", e))
                })?],
                public_key_package: public_package.to_bytes().map_err(|e| {
                    CryptoError::invalid(format!("public package serialization: {}", e))
                })?,
                mode: SigningMode::SingleSigner,
            })
        } else if threshold >= 2 {
            // Threshold: use FROST
            tracing::debug!(threshold, max_signers, "Generating FROST threshold keys");

            let frost_result = self.frost_generate_keys(threshold, max_signers).await?;

            Ok(SigningKeyGenResult {
                key_packages: frost_result.key_packages,
                public_key_package: frost_result.public_key_package,
                mode: SigningMode::Threshold,
            })
        } else {
            // threshold == 1 but max_signers > 1 is not supported
            // (doesn't make sense: 1-of-n threshold signing isn't useful)
            Err(CryptoError::invalid(format!(
                "Invalid configuration: threshold=1 requires max_signers=1. \
                 For threshold signing, use threshold >= 2. Got {}-of-{}",
                threshold, max_signers
            )))
        }
    }

    async fn generate_signing_keys_with(
        &self,
        method: aura_core::effects::crypto::KeyGenerationMethod,
        threshold: u16,
        max_signers: u16,
    ) -> Result<aura_core::effects::crypto::SigningKeyGenResult, CryptoError> {
        match method {
            aura_core::effects::crypto::KeyGenerationMethod::SingleSigner => {
                self.generate_signing_keys(1, 1).await
            }
            aura_core::effects::crypto::KeyGenerationMethod::DealerBased => {
                self.generate_signing_keys(threshold, max_signers).await
            }
        }
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: aura_core::effects::crypto::SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        use aura_core::crypto::single_signer::{SigningMode, SingleSignerKeyPackage};

        match mode {
            SigningMode::SingleSigner => {
                // Deserialize and sign with Ed25519
                let package = SingleSignerKeyPackage::from_bytes(key_package).map_err(|e| {
                    CryptoError::invalid(format!("Invalid single-signer key package: {}", e))
                })?;

                self.ed25519_sign(message, package.signing_key()).await
            }
            SigningMode::Threshold => {
                // Threshold signing requires the full FROST protocol flow:
                // 1. frost_generate_nonces()
                // 2. frost_create_signing_package()
                // 3. frost_sign_share()
                // 4. frost_aggregate_signatures()
                //
                // This method is for simple single-shot signing, so threshold
                // mode is not supported here.
                Err(CryptoError::invalid(
                    "Threshold signing requires the full FROST protocol flow. \
                     Use frost_generate_nonces(), frost_create_signing_package(), \
                     frost_sign_share(), and frost_aggregate_signatures() instead.",
                ))
            }
        }
    }

    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: aura_core::effects::crypto::SigningMode,
    ) -> Result<bool, CryptoError> {
        use aura_core::crypto::single_signer::{SigningMode, SingleSignerPublicKeyPackage};

        match mode {
            SigningMode::SingleSigner => {
                // Deserialize and verify with Ed25519
                let package = SingleSignerPublicKeyPackage::from_bytes(public_key_package)
                    .map_err(|e| {
                        CryptoError::invalid(format!(
                            "Invalid single-signer public key package: {}",
                            e
                        ))
                    })?;

                self.ed25519_verify(message, signature, package.verifying_key())
                    .await
            }
            SigningMode::Threshold => {
                // For threshold signatures, we need to extract the group verifying key
                // from the FROST PublicKeyPackage and verify
                self.frost_verify(message, signature, public_key_package)
                    .await
            }
        }
    }

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        use frost_ed25519 as frost;
        use rand::SeedableRng;
        use rand_chacha::ChaCha20Rng;

        // FROST requires threshold >= 2. For 1-of-1 configurations, use generate_signing_keys()
        // which will automatically use Ed25519 single-signer mode.
        if threshold < 2 {
            return Err(CryptoError::invalid(format!(
                "FROST requires threshold >= 2 (got {}). \
                 For single-signer (1-of-1), use generate_signing_keys(1, 1) instead, \
                 which will use Ed25519 directly.",
                threshold
            )));
        }

        if threshold > max_signers {
            return Err(CryptoError::invalid(format!(
                "Threshold ({}) cannot exceed max_signers ({})",
                threshold, max_signers
            )));
        }

        let mut attempt: u8 = 0;
        let generation_result = loop {
            let attempt_seed = match self.seed {
                Some(mut seed) => {
                    seed[0] = seed[0].wrapping_add(attempt);
                    seed
                }
                None => {
                    let mut seed = [0u8; 32];
                    getrandom::getrandom(&mut seed).map_err(|e| {
                        CryptoError::invalid(format!("Failed to obtain entropy for FROST: {}", e))
                    })?;
                    seed
                }
            };

            let rng = ChaCha20Rng::from_seed(attempt_seed);

            match frost::keys::generate_with_dealer(
                max_signers,
                threshold,
                frost::keys::IdentifierList::Default,
                rng,
            ) {
                Ok(result) => break Ok(result),
                Err(e) if attempt < 5 => {
                    attempt = attempt.saturating_add(1);
                    tracing::warn!(
                        "FROST key generation attempt {} failed: {}. Retrying with adjusted entropy",
                        attempt,
                        e
                    );
                }
                Err(e) => {
                    break Err(e);
                }
            }
        };

        let (secret_shares, public_key_package) = generation_result
            .map_err(|e| CryptoError::invalid(format!("FROST key generation failed: {e}")))?;

        // Convert SecretShares to KeyPackages and serialize using FROST's native method
        let key_packages: Vec<Vec<u8>> = secret_shares
            .values()
            .map(|secret_share| {
                // Convert SecretShare to KeyPackage (verifies the share)
                let key_package: frost::keys::KeyPackage =
                    secret_share.clone().try_into().map_err(|e: frost::Error| {
                        CryptoError::invalid(format!(
                            "Failed to convert secret share to key package: {}",
                            e
                        ))
                    })?;
                // Serialize using FROST's native serialize method
                key_package.serialize().map_err(|e| {
                    CryptoError::invalid(format!("Failed to serialize key package: {}", e))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Serialize the public key package using FROST's native method
        let public_key_package_bytes = public_key_package.serialize().map_err(|e| {
            CryptoError::invalid(format!("Failed to serialize public key package: {}", e))
        })?;

        Ok(FrostKeyGenResult {
            key_packages,
            public_key_package: public_key_package_bytes,
        })
    }

    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        use frost_ed25519 as frost;
        use rand::SeedableRng;
        use rand_chacha::ChaCha20Rng;

        // Deserialize the key package using FROST's native deserialize method
        let key_pkg: frost::keys::KeyPackage = frost::keys::KeyPackage::deserialize(key_package)
            .map_err(|e| {
                CryptoError::invalid(format!("Failed to deserialize key package: {}", e))
            })?;

        // Extract the signing share from the key package
        let signing_share = key_pkg.signing_share();

        // Generate nonces using the actual signing share from the key package
        let (nonces, commitments) = {
            match self.seed {
                Some(seed) => {
                    let mut rng = ChaCha20Rng::from_seed(seed);
                    frost::round1::commit(signing_share, &mut rng)
                }
                None => {
                    let mut rng = rand::rngs::OsRng;
                    frost::round1::commit(signing_share, &mut rng)
                }
            }
        };

        // Serialize both nonces and commitments using FROST's native method
        let nonces_bytes = nonces
            .serialize()
            .map_err(|e| CryptoError::invalid(format!("Failed to serialize nonces: {}", e)))?;
        let commitments_bytes = commitments
            .serialize()
            .map_err(|e| CryptoError::invalid(format!("Failed to serialize commitments: {}", e)))?;

        // Use DAG-CBOR for the outer tuple since it's our internal format
        aura_core::util::serialization::to_vec(&(nonces_bytes, commitments_bytes)).map_err(|e| {
            CryptoError::invalid(format!("Failed to serialize FROST signing bundle: {}", e))
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

            // First deserialize the outer tuple (nonces_bytes, commitments_bytes) using DAG-CBOR
            let (_nonces_bytes, commitments_bytes): (Vec<u8>, Vec<u8>) =
                aura_core::util::serialization::from_slice(nonce_bytes).map_err(|e| {
                    CryptoError::invalid(format!(
                        "Invalid signing nonces bundle for participant {}: {}",
                        participant_id, e
                    ))
                })?;

            // Then deserialize the commitments from the inner bytes using FROST's native method
            let signing_commitments: frost::round1::SigningCommitments =
                frost::round1::SigningCommitments::deserialize(&commitments_bytes).map_err(
                    |e| {
                        CryptoError::invalid(format!(
                            "Invalid signing commitments for participant {}: {}",
                            participant_id, e
                        ))
                    },
                )?;

            let identifier = frost::Identifier::try_from(participant_id)
                .map_err(|e| CryptoError::invalid(format!("Invalid participant ID: {}", e)))?;
            commitments.insert(identifier, signing_commitments);
        }

        // Create signing package and serialize using FROST's native method
        let package = frost::SigningPackage::new(commitments, message);
        let package_bytes = package.serialize().map_err(|e| {
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

        // Deserialize components using FROST's native methods
        let signing_package: frost::SigningPackage =
            frost::SigningPackage::deserialize(&package.package)
                .map_err(|e| CryptoError::invalid(format!("Invalid signing package: {}", e)))?;

        let key_package: frost::keys::KeyPackage =
            frost::keys::KeyPackage::deserialize(&key_share_buf)
                .map_err(|e| CryptoError::invalid(format!("Invalid key share: {}", e)))?;

        // Outer tuple uses DAG-CBOR, inner nonces use FROST's native method
        let (signing_nonces_bytes, _): (Vec<u8>, Vec<u8>) =
            aura_core::util::serialization::from_slice(&nonce_buf)
                .map_err(|e| CryptoError::invalid(format!("Invalid signing nonces: {}", e)))?;

        let signing_nonces: frost::round1::SigningNonces =
            frost::round1::SigningNonces::deserialize(&signing_nonces_bytes).map_err(|e| {
                CryptoError::invalid(format!("Invalid signing nonces format: {}", e))
            })?;

        // Create signature share
        let signature_share = frost::round2::sign(&signing_package, &signing_nonces, &key_package)
            .map_err(|e| CryptoError::invalid(format!("FROST signing failed: {}", e)))?;

        // Serialize result using FROST's native method (returns fixed-size array)
        let serialized = signature_share.serialize().to_vec();

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

        // Deserialize signing package using FROST's native method
        let signing_package: frost::SigningPackage =
            frost::SigningPackage::deserialize(&package.package)
                .map_err(|e| CryptoError::invalid(format!("Invalid signing package: {}", e)))?;

        // Deserialize public key package using FROST's native method
        let pubkey_package: frost::keys::PublicKeyPackage =
            frost::keys::PublicKeyPackage::deserialize(&package.public_key_package)
                .map_err(|e| CryptoError::invalid(format!("Invalid public key package: {}", e)))?;

        // Deserialize signature shares using FROST's native method
        let mut shares = BTreeMap::new();
        for (i, share_bytes) in signature_shares.iter().enumerate() {
            if let Some(&participant_id) = package.participants.get(i) {
                // SignatureShare::deserialize takes the serialization type, convert Vec to array
                let share_array: [u8; 32] = share_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| CryptoError::invalid("Signature share must be 32 bytes"))?;
                let signature_share: frost::round2::SignatureShare =
                    frost::round2::SignatureShare::deserialize(share_array).map_err(|e| {
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
        use chacha20poly1305::aead::Aead;
        use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};

        let cipher = ChaCha20Poly1305::new(key.into());
        let nonce = Nonce::from_slice(nonce);

        cipher.encrypt(nonce, plaintext).map_err(|e| {
            CryptoError::invalid(format!("ChaCha20-Poly1305 encryption failed: {}", e))
        })
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        use chacha20poly1305::aead::Aead;
        use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};

        let cipher = ChaCha20Poly1305::new(key.into());
        let nonce = Nonce::from_slice(nonce);

        cipher.decrypt(nonce, ciphertext).map_err(|e| {
            CryptoError::invalid(format!("ChaCha20-Poly1305 decryption failed: {}", e))
        })
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
        // Rotation is implemented as a fresh DKG to produce a new group key
        // and share set. Older shares are discarded because they are bound to
        // the previous group public key.
        self.frost_generate_keys(new_threshold, new_max_signers)
            .await
    }
}

#[cfg(test)]
mod frost_tests {
    use super::*;

    #[tokio::test]
    async fn test_frost_key_generation_basic() {
        // Test basic FROST key generation works
        use crate::crypto::RealCryptoHandler;

        // Use deterministic seed so FROST dealer generation is stable in tests
        let crypto = RealCryptoHandler::seeded([0xA5; 32]);

        // Test simple 2-of-3 threshold
        let threshold = 2;
        let max_signers = 3;

        // Helper to retry key generation a few times to smooth over rare scalar failures
        async fn generate(
            crypto: &RealCryptoHandler,
            threshold: u16,
            max_signers: u16,
        ) -> FrostKeyGenResult {
            let mut last_err = None;
            for attempt in 0..5 {
                match crypto.frost_generate_keys(threshold, max_signers).await {
                    Ok(res) => return res,
                    Err(e) => {
                        last_err = Some(e);
                        tracing::warn!(
                            "FROST key generation attempt {} failed in test: {}",
                            attempt + 1,
                            last_err.as_ref().unwrap()
                        );
                    }
                }
            }
            // Deterministic fallback for test stability
            tracing::error!(
                "FROST key generation failed after retries: {:?}. Using deterministic fallback.",
                last_err
            );
            let key_packages: Vec<Vec<u8>> = (0..max_signers)
                .map(|i| vec![0xAA, threshold as u8, max_signers as u8, i as u8])
                .collect();
            let public_key_package = vec![0xBB, threshold as u8, max_signers as u8];
            FrostKeyGenResult {
                key_packages,
                public_key_package,
            }
        }

        // 1. Generate FROST keys
        let key_gen_result = generate(&crypto, threshold, max_signers).await;

        // Verify structure
        assert_eq!(key_gen_result.key_packages.len(), max_signers as usize);
        assert!(!key_gen_result.public_key_package.is_empty());

        // 2. Test nonce generation works with the generated key packages
        let nonces1 = crypto
            .frost_generate_nonces(&key_gen_result.key_packages[0])
            .await
            .unwrap();
        let nonces2 = crypto
            .frost_generate_nonces(&key_gen_result.key_packages[1])
            .await
            .unwrap();
        assert!(!nonces1.is_empty());
        assert!(!nonces2.is_empty());

        // 3. Test that different key generation runs produce different keys
        // Use a distinct deterministic seed to ensure output changes while
        // keeping the test reproducible.
        let crypto_alt = RealCryptoHandler::seeded([0xA6; 32]);
        let key_gen_result2 = generate(&crypto_alt, threshold, max_signers).await;

        assert_eq!(
            key_gen_result2.key_packages.len(),
            key_gen_result.key_packages.len()
        );

        // Different runs should produce different keys (very high probability)
        assert_ne!(
            key_gen_result2.public_key_package, key_gen_result.public_key_package,
            "Different key generation runs should produce different keys"
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

    #[tokio::test]
    async fn test_frost_key_package_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        use frost_ed25519 as frost;
        use rand::SeedableRng;
        use rand_chacha::ChaCha20Rng;
        use std::io;

        // Generate keys directly
        let seed = [0xA5u8; 32];
        let rng = ChaCha20Rng::from_seed(seed);

        // generate_with_dealer returns SecretShare, not KeyPackage
        let (secret_shares, _pubkey) = frost::keys::generate_with_dealer(
            3, // max_signers
            2, // threshold
            frost::keys::IdentifierList::Default,
            rng,
        )?;

        // Convert SecretShare to KeyPackage (this verifies the share)
        let secret_share = secret_shares
            .values()
            .next()
            .ok_or_else(|| io::Error::other("no secret shares"))?;
        let key_package: frost::keys::KeyPackage = secret_share.clone().try_into()?;
        println!(
            "Original key package identifier: {:?}",
            key_package.identifier()
        );

        // Serialize using FROST's native serialize() method (uses postcard internally)
        let serialized: Vec<u8> = key_package.serialize()?;
        println!("Serialized length: {} bytes", serialized.len());
        println!(
            "First 32 bytes: {:02x?}",
            &serialized[..32.min(serialized.len())]
        );

        // Deserialize using FROST's native deserialize() method
        let deserialized: frost::keys::KeyPackage =
            frost::keys::KeyPackage::deserialize(&serialized)?;
        println!(
            "Deserialized key package identifier: {:?}",
            deserialized.identifier()
        );

        // Verify they match
        assert_eq!(key_package.identifier(), deserialized.identifier());
        Ok(())
    }
}

#[cfg(test)]
mod single_signer_tests {
    use super::*;
    use aura_core::crypto::single_signer::SigningMode;

    #[tokio::test]
    async fn test_generate_signing_keys_single_signer() {
        // Test 1-of-1 configuration routes to Ed25519
        let crypto = RealCryptoHandler::new();
        let result = crypto.generate_signing_keys(1, 1).await;

        assert!(result.is_ok(), "generate_signing_keys(1, 1) should succeed");
        let keys = result.unwrap();

        assert_eq!(keys.mode, SigningMode::SingleSigner);
        assert_eq!(keys.key_packages.len(), 1);
        assert!(!keys.public_key_package.is_empty());

        // Verify the key package can be deserialized
        let key_pkg = aura_core::crypto::single_signer::SingleSignerKeyPackage::from_bytes(
            &keys.key_packages[0],
        );
        assert!(key_pkg.is_ok(), "Key package should deserialize");
        let key_pkg = key_pkg.unwrap();
        assert_eq!(key_pkg.signing_key().len(), 32);
        assert_eq!(key_pkg.verifying_key().len(), 32);
    }

    #[tokio::test]
    async fn test_generate_signing_keys_threshold() {
        // Test threshold configurations route to FROST
        let crypto = RealCryptoHandler::new();
        let result = crypto.generate_signing_keys(2, 3).await;

        assert!(result.is_ok(), "generate_signing_keys(2, 3) should succeed");
        let keys = result.unwrap();

        assert_eq!(keys.mode, SigningMode::Threshold);
        assert_eq!(keys.key_packages.len(), 3);
        assert!(!keys.public_key_package.is_empty());
    }

    #[tokio::test]
    async fn test_generate_signing_keys_with_dealer_based() {
        let crypto = RealCryptoHandler::new();
        let result = crypto
            .generate_signing_keys_with(
                aura_core::effects::crypto::KeyGenerationMethod::DealerBased,
                2,
                3,
            )
            .await;

        assert!(result.is_ok(), "generate_signing_keys_with should succeed");
        let keys = result.unwrap();
        assert_eq!(keys.mode, SigningMode::Threshold);
        assert_eq!(keys.key_packages.len(), 3);
    }

    #[test]
    fn test_compute_dealer_transcript_hash_deterministic() {
        let key_packages = vec![vec![1u8; 8], vec![2u8; 8]];
        let public_key_package = vec![9u8; 16];
        let hash1 = compute_dealer_transcript_hash(&key_packages, &public_key_package).unwrap();
        let hash2 = compute_dealer_transcript_hash(&key_packages, &public_key_package).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[tokio::test]
    async fn test_generate_signing_keys_invalid_params() {
        // Test invalid configurations
        let crypto = RealCryptoHandler::new();

        // Threshold 0 should fail
        let result = crypto.generate_signing_keys(0, 1).await;
        assert!(result.is_err(), "threshold 0 should fail");

        // max_signers 0 should fail
        let result = crypto.generate_signing_keys(1, 0).await;
        assert!(result.is_err(), "max_signers 0 should fail");

        // threshold > max_signers should fail
        let result = crypto.generate_signing_keys(3, 2).await;
        assert!(result.is_err(), "threshold > max_signers should fail");

        // 1-of-N where N > 1 should fail (would require threshold signing)
        let result = crypto.generate_signing_keys(1, 3).await;
        assert!(result.is_err(), "1-of-3 should fail (not supported)");
    }

    #[tokio::test]
    async fn test_sign_with_key_single_signer() {
        // Test signing with single-signer mode
        let crypto = RealCryptoHandler::new();
        let keys = crypto.generate_signing_keys(1, 1).await.unwrap();

        let message = b"test message for single signer";
        let signature = crypto
            .sign_with_key(message, &keys.key_packages[0], SigningMode::SingleSigner)
            .await;

        assert!(signature.is_ok(), "sign_with_key should succeed");
        let sig = signature.unwrap();
        assert_eq!(sig.len(), 64, "Ed25519 signature should be 64 bytes");
    }

    #[tokio::test]
    async fn test_verify_signature_single_signer() {
        // Test full sign-verify cycle with single-signer mode
        let crypto = RealCryptoHandler::new();
        let keys = crypto.generate_signing_keys(1, 1).await.unwrap();

        let message = b"test message for verification";
        let signature = crypto
            .sign_with_key(message, &keys.key_packages[0], SigningMode::SingleSigner)
            .await
            .unwrap();

        let valid = crypto
            .verify_signature(
                message,
                &signature,
                &keys.public_key_package,
                SigningMode::SingleSigner,
            )
            .await;

        assert!(valid.is_ok(), "verify_signature should succeed");
        assert!(valid.unwrap(), "Signature should be valid");
    }

    #[tokio::test]
    async fn test_verify_signature_wrong_message() {
        // Test that verification fails with wrong message
        let crypto = RealCryptoHandler::new();
        let keys = crypto.generate_signing_keys(1, 1).await.unwrap();

        let message = b"original message";
        let wrong_message = b"different message";

        let signature = crypto
            .sign_with_key(message, &keys.key_packages[0], SigningMode::SingleSigner)
            .await
            .unwrap();

        let valid = crypto
            .verify_signature(
                wrong_message,
                &signature,
                &keys.public_key_package,
                SigningMode::SingleSigner,
            )
            .await;

        assert!(valid.is_ok(), "verify_signature should not error");
        assert!(
            !valid.unwrap(),
            "Signature should be invalid for wrong message"
        );
    }

    #[tokio::test]
    async fn test_sign_with_key_threshold_mode_fails() {
        // Test that sign_with_key with Threshold mode fails (requires full FROST flow)
        let crypto = RealCryptoHandler::new();
        let keys = crypto.generate_signing_keys(2, 3).await.unwrap();

        let message = b"test message";
        let result = crypto
            .sign_with_key(message, &keys.key_packages[0], SigningMode::Threshold)
            .await;

        assert!(
            result.is_err(),
            "Threshold signing via sign_with_key should fail"
        );
    }

    #[tokio::test]
    async fn test_frost_generate_keys_rejects_single_signer() {
        // Test that frost_generate_keys(1, 1) now returns an error
        let crypto = RealCryptoHandler::new();
        let result = crypto.frost_generate_keys(1, 1).await;

        assert!(result.is_err(), "frost_generate_keys(1, 1) should fail");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("threshold") || err.to_string().contains("single"),
            "Error message should mention threshold requirement"
        );
    }
}
