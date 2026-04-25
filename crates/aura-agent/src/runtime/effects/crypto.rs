use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::crypto::single_signer::SigningMode;
use aura_core::crypto::tree_signing;
use aura_core::effects::crypto::{
    FrostKeyGenResult, FrostSigningPackage, KeyDerivationContext, KeyGenerationMethod,
    SigningKeyGenResult,
};
use aura_core::effects::{
    CryptoCoreEffects, CryptoError, CryptoExtendedEffects, RandomCoreEffects,
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::threshold::ParticipantIdentity;
use aura_core::{AuraError, AuthorityId};
use aura_signature::threshold_signing_context_transcript_bytes;
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use serde::{Deserialize, Serialize};

const PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION: u8 = 1;
const PARTICIPANT_KEY_PACKAGE_AAD_DOMAIN: &str = "aura:participant-key-package-envelope:v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ParticipantKeyPackageEnvelope {
    version: u8,
    authority: AuthorityId,
    epoch: u64,
    recipient: ParticipantIdentity,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

impl AuraEffectSystem {
    fn participant_share_location(
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}:{}", authority, epoch),
            participant.storage_key(),
        )
    }

    fn participant_wrap_key_location(
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key(
            "participant_share_wrap_keys",
            format!("{}:{}", authority, epoch),
            participant.storage_key(),
        )
    }

    fn participant_key_package_aad(
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> Vec<u8> {
        format!(
            "{}:{}:{}:{}",
            PARTICIPANT_KEY_PACKAGE_AAD_DOMAIN,
            authority,
            epoch,
            participant.storage_key()
        )
        .into_bytes()
    }

    async fn load_or_create_participant_wrap_key(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> Result<[u8; 32], AuraError> {
        let location = Self::participant_wrap_key_location(authority, epoch, participant);
        let caps = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        match self
            .crypto
            .secure_storage()
            .secure_retrieve(&location, &caps)
            .await
        {
            Ok(bytes) => bytes.try_into().map_err(|_| {
                AuraError::internal("participant share wrapping key has invalid length")
            }),
            Err(_) => {
                let key = self.random_bytes_32().await;
                self.crypto
                    .secure_storage()
                    .secure_store(&location, &key, &caps)
                    .await?;
                Ok(key)
            }
        }
    }

    async fn encrypt_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        key_package: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        let wrap_key = self
            .load_or_create_participant_wrap_key(authority, epoch, participant)
            .await?;
        let cipher = ChaCha20Poly1305::new((&wrap_key).into());
        let nonce = self.random_bytes(12).await;
        let aad = Self::participant_key_package_aad(authority, epoch, participant);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: key_package,
                    aad: &aad,
                },
            )
            .map_err(|e| AuraError::internal(format!("Failed to encrypt key package: {e}")))?;
        let envelope = ParticipantKeyPackageEnvelope {
            version: PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION,
            authority: *authority,
            epoch,
            recipient: participant.clone(),
            nonce,
            ciphertext,
        };
        serde_json::to_vec(&envelope).map_err(|e| {
            AuraError::internal(format!("Failed to serialize key package envelope: {e}"))
        })
    }

    async fn decrypt_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        envelope_bytes: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        let envelope: ParticipantKeyPackageEnvelope = serde_json::from_slice(envelope_bytes)
            .map_err(|e| AuraError::internal(format!("Invalid key package envelope: {e}")))?;
        if envelope.version != PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION
            || envelope.authority != *authority
            || envelope.epoch != epoch
            || envelope.recipient != *participant
            || envelope.nonce.len() != 12
        {
            return Err(AuraError::internal(
                "key package envelope metadata does not match storage location",
            ));
        }
        let wrap_key = self
            .load_or_create_participant_wrap_key(authority, epoch, participant)
            .await?;
        let cipher = ChaCha20Poly1305::new((&wrap_key).into());
        let aad = Self::participant_key_package_aad(authority, epoch, participant);
        cipher
            .decrypt(
                Nonce::from_slice(&envelope.nonce),
                Payload {
                    msg: &envelope.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|e| AuraError::internal(format!("Failed to decrypt key package: {e}")))
    }

    async fn signing_mode_for_epoch(&self, authority: &AuthorityId, epoch: u64) -> SigningMode {
        match self.get_threshold_config_metadata(authority, epoch).await {
            Some(metadata) if metadata.threshold_k > 1 => SigningMode::Threshold,
            _ => SigningMode::SingleSigner,
        }
    }

    fn solo_signing_key_location(authority: &AuthorityId, epoch: u64) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key("signing_keys", format!("{}:{}", authority, epoch), "1")
    }

    fn solo_public_key_location(authority: &AuthorityId, epoch: u64) -> SecureStorageLocation {
        SecureStorageLocation::new("signing_keys_public", format!("{}:{}", authority, epoch))
    }

    fn threshold_public_key_location(authority: &AuthorityId, epoch: u64) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", epoch),
        )
    }
}

// Implementation of RandomCoreEffects
#[async_trait]
impl RandomCoreEffects for AuraEffectSystem {
    #[allow(clippy::disallowed_methods)]
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.crypto.random_bytes(len)
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_bytes_32(&self) -> [u8; 32] {
        self.crypto.random_32_bytes()
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_u64(&self) -> u64 {
        self.crypto.random_u64()
    }
}

// Implementation of CryptoCoreEffects
#[async_trait]
impl CryptoCoreEffects for AuraEffectSystem {
    async fn kdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: u32,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .kdf_derive(ikm, salt, info, output_len)
            .await
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler().derive_key(master_key, context).await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        self.crypto.handler().ed25519_generate_keypair().await
    }

    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .ed25519_sign(message, private_key)
            .await
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto
            .handler()
            .ed25519_verify(message, signature, public_key)
            .await
    }

    fn is_simulated(&self) -> bool {
        self.crypto.handler().is_simulated()
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        self.crypto.handler().crypto_capabilities()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.crypto.handler().constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.crypto.handler().secure_zero(data);
    }
}

// Implementation of CryptoExtendedEffects
#[async_trait]
impl CryptoExtendedEffects for AuraEffectSystem {
    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        self.crypto
            .handler()
            .frost_generate_keys(threshold, max_signers)
            .await
    }

    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .frost_generate_nonces(key_package)
            .await
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<FrostSigningPackage, CryptoError> {
        self.crypto
            .handler()
            .frost_create_signing_package(message, nonces, participants, public_key_package)
            .await
    }

    async fn frost_sign_share(
        &self,
        signing_package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .frost_sign_share(signing_package, key_share, nonces)
            .await
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .frost_aggregate_signatures(signing_package, signature_shares)
            .await
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto
            .handler()
            .frost_verify(message, signature, public_key)
            .await
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler().ed25519_public_key(private_key).await
    }

    async fn convert_ed25519_to_x25519_public(
        &self,
        ed25519_public_key: &[u8],
    ) -> Result<[u8; 32], CryptoError> {
        self.crypto
            .handler()
            .convert_ed25519_to_x25519_public(ed25519_public_key)
            .await
    }

    async fn convert_ed25519_to_x25519_private(
        &self,
        ed25519_private_key: &[u8],
    ) -> Result<[u8; 32], CryptoError> {
        self.crypto
            .handler()
            .convert_ed25519_to_x25519_private(ed25519_private_key)
            .await
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .chacha20_encrypt(plaintext, key, nonce)
            .await
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .chacha20_decrypt(ciphertext, key, nonce)
            .await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .aes_gcm_encrypt(plaintext, key, nonce)
            .await
    }

    async fn aes_gcm_encrypt_with_aad(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .aes_gcm_encrypt_with_aad(plaintext, key, nonce, aad)
            .await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .aes_gcm_decrypt(ciphertext, key, nonce)
            .await
    }

    async fn aes_gcm_decrypt_with_aad(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .aes_gcm_decrypt_with_aad(ciphertext, key, nonce, aad)
            .await
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        self.crypto
            .handler()
            .frost_rotate_keys(old_shares, old_threshold, new_threshold, new_max_signers)
            .await
    }

    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.crypto
            .handler()
            .generate_signing_keys(threshold, max_signers)
            .await
    }

    async fn generate_signing_keys_with(
        &self,
        method: KeyGenerationMethod,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.crypto
            .handler()
            .generate_signing_keys_with(method, threshold, max_signers)
            .await
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .sign_with_key(message, key_package, mode)
            .await
    }

    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        self.crypto
            .handler()
            .verify_signature(message, signature, public_key_package, mode)
            .await
    }
}

// Implementation of ThresholdSigningEffects
#[async_trait]
impl aura_core::effects::ThresholdSigningEffects for AuraEffectSystem {
    async fn bootstrap_authority(&self, authority: &AuthorityId) -> Result<Vec<u8>, AuraError> {
        // Generate 1-of-1 signing keys (uses Ed25519 for single-signer mode)
        let signing_keys = self.crypto.handler().generate_signing_keys(1, 1).await?;

        // Store key package in secure storage
        // Location varies by mode: signing_keys/ for Ed25519, frost_keys/ for FROST
        let key_prefix = match signing_keys.mode {
            SigningMode::SingleSigner => "signing_keys",
            SigningMode::Threshold => "frost_keys",
        };
        let location = SecureStorageLocation::with_sub_key(
            key_prefix,
            format!("{}:0", authority), // epoch 0
            "1",                        // signer index 1
        );
        let caps = vec![SecureStorageCapability::Write];
        let participant = ParticipantIdentity::guardian(*authority);
        let key_package_envelope = self
            .encrypt_participant_key_package(
                authority,
                0,
                &participant,
                &signing_keys.key_packages[0],
            )
            .await?;
        self.crypto
            .secure_storage()
            .secure_store(&location, &key_package_envelope, &caps)
            .await?;

        // Store public key package in both the legacy single-signer path and the
        // canonical threshold path so runtime bootstrap implementations share one layout.
        let pub_location = SecureStorageLocation::new(
            format!("{}_public", key_prefix),
            format!("{}:0", authority),
        );
        self.crypto
            .secure_storage()
            .secure_store(&pub_location, &signing_keys.public_key_package, &caps)
            .await?;
        self.crypto
            .secure_storage()
            .secure_store(
                &Self::threshold_public_key_location(authority, 0),
                &signing_keys.public_key_package,
                &caps,
            )
            .await?;

        // Store threshold config metadata for epoch 0 (bootstrap case: 1-of-1 single signer)
        self.store_threshold_config_metadata(
            authority,
            0,   // epoch 0
            1,   // threshold
            1,   // total_participants
            &[], // 1-of-1 bootstrap: participant set is implicit (local signer)
            aura_core::threshold::AgreementMode::Provisional,
        )
        .await?;

        // Bootstrap Biscuit authorization tokens
        self.bootstrap_biscuit_tokens(authority).await?;

        let (_key_packages, public_key_package, _mode) = signing_keys.into_parts();
        Ok(public_key_package)
    }

    async fn sign(
        &self,
        context: aura_core::threshold::SigningContext,
    ) -> Result<aura_core::threshold::ThresholdSignature, AuraError> {
        let current_epoch = self.get_current_epoch(&context.authority).await;
        let message =
            threshold_signing_context_transcript_bytes(&context, current_epoch).map_err(|e| {
                AuraError::internal(format!("Failed to encode signing context transcript: {e}"))
            })?;
        let caps = vec![SecureStorageCapability::Read];
        let mode = self
            .signing_mode_for_epoch(&context.authority, current_epoch)
            .await;

        match mode {
            SigningMode::SingleSigner => {
                let key_location =
                    Self::solo_signing_key_location(&context.authority, current_epoch);
                let key_package = self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(&key_location, &caps)
                    .await?;
                let participant = ParticipantIdentity::guardian(context.authority);
                let key_package = self
                    .decrypt_participant_key_package(
                        &context.authority,
                        current_epoch,
                        &participant,
                        &key_package,
                    )
                    .await?;

                let public_key_package = match self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(
                        &Self::solo_public_key_location(&context.authority, current_epoch),
                        &caps,
                    )
                    .await
                {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        self.crypto
                            .secure_storage()
                            .secure_retrieve(
                                &Self::threshold_public_key_location(
                                    &context.authority,
                                    current_epoch,
                                ),
                                &caps,
                            )
                            .await?
                    }
                };

                let signature = self
                    .crypto
                    .handler()
                    .sign_with_key(&message, &key_package, SigningMode::SingleSigner)
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Single-signer signing failed: {e}"))
                    })?;

                Ok(aura_core::threshold::ThresholdSignature::single_signer(
                    signature,
                    public_key_package,
                    current_epoch,
                ))
            }
            SigningMode::Threshold => {
                let participant = ParticipantIdentity::guardian(context.authority);
                let key_package = self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(
                        &Self::participant_share_location(
                            &context.authority,
                            current_epoch,
                            &participant,
                        ),
                        &caps,
                    )
                    .await?;
                let key_package = self
                    .decrypt_participant_key_package(
                        &context.authority,
                        current_epoch,
                        &participant,
                        &key_package,
                    )
                    .await?;
                let public_key_package = self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(
                        &Self::threshold_public_key_location(&context.authority, current_epoch),
                        &caps,
                    )
                    .await?;

                let share =
                    tree_signing::share_from_key_package_bytes(&key_package).map_err(|e| {
                        AuraError::internal(format!("Failed to decode threshold key package: {e}"))
                    })?;
                let nonces = self
                    .crypto
                    .handler()
                    .frost_generate_nonces(&key_package)
                    .await
                    .map_err(|e| AuraError::internal(format!("Nonce generation failed: {e}")))?;
                let participants = vec![share.identifier];
                let signing_package = self
                    .crypto
                    .handler()
                    .frost_create_signing_package(
                        &message,
                        std::slice::from_ref(&nonces),
                        &participants,
                        &public_key_package,
                    )
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Signing package creation failed: {e}"))
                    })?;
                let partial = self
                    .crypto
                    .handler()
                    .frost_sign_share(&signing_package, &key_package, &nonces)
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Signature share creation failed: {e}"))
                    })?;
                let signature = self
                    .crypto
                    .handler()
                    .frost_aggregate_signatures(&signing_package, &[partial])
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Signature aggregation failed: {e}"))
                    })?;

                Ok(aura_core::threshold::ThresholdSignature::new(
                    signature,
                    1,
                    participants,
                    public_key_package,
                    current_epoch,
                ))
            }
        }
    }

    async fn threshold_config(
        &self,
        authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdConfig> {
        // Get current epoch for this authority
        let current_epoch = self.get_current_epoch(authority).await;

        // Retrieve stored threshold config metadata for this epoch
        self.get_threshold_config_metadata(authority, current_epoch)
            .await
            .map(|metadata| aura_core::threshold::ThresholdConfig {
                threshold: metadata.threshold_k,
                total_participants: metadata.total_n,
            })
    }

    async fn threshold_state(
        &self,
        authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdState> {
        // Get current epoch for this authority
        let current_epoch = self.get_current_epoch(authority).await;

        // Retrieve stored threshold config metadata for this epoch
        self.get_threshold_config_metadata(authority, current_epoch)
            .await
            .map(|metadata| aura_core::threshold::ThresholdState {
                epoch: current_epoch,
                threshold: metadata.threshold_k,
                total_participants: metadata.total_n,
                participants: metadata.resolved_participants(),
                agreement_mode: metadata.agreement_mode,
            })
    }

    async fn has_signing_capability(&self, authority: &AuthorityId) -> bool {
        let current_epoch = self.get_current_epoch(authority).await;
        let location = match self.signing_mode_for_epoch(authority, current_epoch).await {
            SigningMode::SingleSigner => Self::solo_signing_key_location(authority, current_epoch),
            SigningMode::Threshold => Self::participant_share_location(
                authority,
                current_epoch,
                &ParticipantIdentity::guardian(*authority),
            ),
        };
        self.crypto
            .secure_storage()
            .secure_exists(&location)
            .await
            .unwrap_or(false)
    }

    async fn public_key_package(&self, authority: &AuthorityId) -> Option<Vec<u8>> {
        let current_epoch = self.get_current_epoch(authority).await;
        let caps = vec![SecureStorageCapability::Read];
        match self.signing_mode_for_epoch(authority, current_epoch).await {
            SigningMode::SingleSigner => match self
                .crypto
                .secure_storage()
                .secure_retrieve(
                    &Self::solo_public_key_location(authority, current_epoch),
                    &caps,
                )
                .await
            {
                Ok(bytes) => Some(bytes),
                Err(_) => self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(
                        &Self::threshold_public_key_location(authority, current_epoch),
                        &caps,
                    )
                    .await
                    .ok(),
            },
            SigningMode::Threshold => self
                .crypto
                .secure_storage()
                .secure_retrieve(
                    &Self::threshold_public_key_location(authority, current_epoch),
                    &caps,
                )
                .await
                .ok(),
        }
    }

    async fn rotate_keys(
        &self,
        authority: &AuthorityId,
        new_threshold: u16,
        new_total_participants: u16,
        participants: &[aura_core::threshold::ParticipantIdentity],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), AuraError> {
        tracing::info!(
            ?authority,
            new_threshold,
            new_total_participants,
            num_participants = participants.len(),
            "Rotating threshold keys via AuraEffectSystem"
        );

        // Validate inputs
        if participants.len() != new_total_participants as usize {
            return Err(AuraError::invalid(format!(
                "Participant count ({}) must match total_participants ({})",
                participants.len(),
                new_total_participants
            )));
        }

        // Get current epoch and calculate new epoch
        let current_epoch = self.get_current_epoch(authority).await;
        let new_epoch = current_epoch + 1;
        tracing::debug!(
            ?authority,
            current_epoch,
            new_epoch,
            "Rotating keys from epoch {} to {}",
            current_epoch,
            new_epoch
        );

        // Generate new threshold keys
        let key_result = if new_threshold >= 2 {
            self.crypto
                .handler()
                .frost_rotate_keys(&[], 0, new_threshold, new_total_participants)
                .await?
        } else {
            let result = self
                .crypto
                .handler()
                .generate_signing_keys(new_threshold, new_total_participants)
                .await?;
            let (key_packages, public_key_package, _mode) = result.into_parts();
            FrostKeyGenResult {
                key_packages,
                public_key_package,
            }
        };

        // Store guardian key packages
        let caps = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        for (participant, key_package) in participants.iter().zip(key_result.key_packages.iter()) {
            let location = Self::participant_share_location(authority, new_epoch, participant);
            let envelope = self
                .encrypt_participant_key_package(authority, new_epoch, participant, key_package)
                .await?;
            self.crypto
                .secure_storage()
                .secure_store(&location, &envelope, &caps)
                .await?;
        }

        // Store public key package
        let pub_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", new_epoch),
        );
        self.crypto
            .secure_storage()
            .secure_store(&pub_location, &key_result.public_key_package, &caps)
            .await?;

        // Store threshold config metadata for the new epoch
        self.store_threshold_config_metadata(
            authority,
            new_epoch,
            new_threshold,
            new_total_participants,
            participants,
            aura_core::threshold::AgreementMode::CoordinatorSoftSafe,
        )
        .await?;

        let (key_packages, public_key_package) = key_result.into_parts();
        Ok((new_epoch, key_packages, public_key_package))
    }

    async fn commit_key_rotation(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
    ) -> Result<(), AuraError> {
        tracing::info!(
            ?authority,
            new_epoch,
            "Committing key rotation via AuraEffectSystem"
        );
        // Activate the new epoch by updating the current epoch state
        self.set_current_epoch(authority, new_epoch).await?;
        tracing::debug!(
            ?authority,
            new_epoch,
            "Epoch state updated - new keys are now active"
        );
        Ok(())
    }

    async fn rollback_key_rotation(
        &self,
        authority: &AuthorityId,
        failed_epoch: u64,
    ) -> Result<(), AuraError> {
        tracing::warn!(
            ?authority,
            failed_epoch,
            "Rolling back key rotation via AuraEffectSystem"
        );
        // Delete orphaned keys from the failed epoch to prevent storage leakage
        self.delete_epoch_keys(authority, failed_epoch).await?;
        tracing::info!(
            ?authority,
            failed_epoch,
            "Successfully deleted orphaned keys from failed rotation"
        );
        Ok(())
    }
}
