use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::crypto::{
    FrostKeyGenResult, FrostSigningPackage, KeyDerivationContext, KeyGenerationMethod,
    SigningKeyGenResult,
};
use aura_core::effects::{
    CryptoCoreEffects, CryptoError, CryptoExtendedEffects, RandomCoreEffects,
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::{AuraError, AuthorityId};

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
    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: u32,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler()
            .hkdf_derive(ikm, salt, info, output_len)
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
        self.crypto.handler().ed25519_sign(message, private_key).await
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto.handler()
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
        self.crypto.handler().secure_zero(data)
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
        self.crypto.handler()
            .frost_generate_keys(threshold, max_signers)
            .await
    }

    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler().frost_generate_nonces(key_package).await
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<FrostSigningPackage, CryptoError> {
        self.crypto.handler()
            .frost_create_signing_package(message, nonces, participants, public_key_package)
            .await
    }

    async fn frost_sign_share(
        &self,
        signing_package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler()
            .frost_sign_share(signing_package, key_share, nonces)
            .await
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler()
            .frost_aggregate_signatures(signing_package, signature_shares)
            .await
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto.handler()
            .frost_verify(message, signature, public_key)
            .await
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler().ed25519_public_key(private_key).await
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler()
            .chacha20_encrypt(plaintext, key, nonce)
            .await
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler()
            .chacha20_decrypt(ciphertext, key, nonce)
            .await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler()
            .aes_gcm_encrypt(plaintext, key, nonce)
            .await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler()
            .aes_gcm_decrypt(ciphertext, key, nonce)
            .await
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        self.crypto.handler()
            .frost_rotate_keys(old_shares, old_threshold, new_threshold, new_max_signers)
            .await
    }

    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.crypto.handler()
            .generate_signing_keys(threshold, max_signers)
            .await
    }

    async fn generate_signing_keys_with(
        &self,
        method: KeyGenerationMethod,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.crypto.handler()
            .generate_signing_keys_with(method, threshold, max_signers)
            .await
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler()
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
        self.crypto.handler()
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
            format!("{}/0", authority), // epoch 0
            "1",                        // signer index 1
        );
        let caps = vec![SecureStorageCapability::Write];
        self.crypto.secure_storage()
            .secure_store(&location, &signing_keys.key_packages[0], &caps)
            .await?;

        // Store public key package
        let pub_location = SecureStorageLocation::new(
            format!("{}_public", key_prefix),
            format!("{}/0", authority),
        );
        self.crypto.secure_storage()
            .secure_store(&pub_location, &signing_keys.public_key_package, &caps)
            .await?;

        // Store threshold metadata for epoch 0 (bootstrap case: 1-of-1 single signer)
        self.store_threshold_metadata(
            authority,
            0,   // epoch 0
            1,   // threshold
            1,   // total_participants
            &[], // 1-of-1 bootstrap: participant set is implicit (local signer)
            aura_core::threshold::AgreementMode::Provisional,
        )
        .await?;

        Ok(signing_keys.public_key_package)
    }

    async fn sign(
        &self,
        context: aura_core::threshold::SigningContext,
    ) -> Result<aura_core::threshold::ThresholdSignature, AuraError> {
        // Serialize the operation for signing
        let message = serde_json::to_vec(&context.operation)
            .map_err(|e| AuraError::internal(format!("Failed to serialize operation: {}", e)))?;

        // Load key package from secure storage using tracked epoch
        let current_epoch = self.get_current_epoch(&context.authority).await;
        let location = SecureStorageLocation::with_sub_key(
            "frost_keys",
            format!("{}/{}", context.authority, current_epoch),
            "1",
        );
        let caps = vec![SecureStorageCapability::Read];
        let key_package = self
            .crypto
            .secure_storage()
            .secure_retrieve(&location, &caps)
            .await?;

        // Load public key package for current epoch
        let pub_location = SecureStorageLocation::new(
            "frost_public_keys",
            format!("{}/{}", context.authority, current_epoch),
        );
        let public_key_package = self
            .crypto
            .secure_storage()
            .secure_retrieve(&pub_location, &caps)
            .await
            .unwrap_or_else(|_| vec![0u8; 32]); // Fallback for bootstrapped authorities

        // Generate nonces
        let nonces = self
            .crypto
            .handler()
            .frost_generate_nonces(&key_package)
            .await
            .map_err(|e| AuraError::internal(format!("Nonce generation failed: {}", e)))?;

        // Create signing package (single participant)
        let participants = vec![1u16];
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
            .map_err(|e| AuraError::internal(format!("Signing package creation failed: {}", e)))?;

        // Sign
        let share = self
            .crypto
            .handler()
            .frost_sign_share(&signing_package, &key_package, &nonces)
            .await
            .map_err(|e| AuraError::internal(format!("Signature share creation failed: {}", e)))?;

        // Aggregate (trivial for single signer)
        let signature = self
            .crypto
            .handler()
            .frost_aggregate_signatures(&signing_package, &[share])
            .await
            .map_err(|e| AuraError::internal(format!("Signature aggregation failed: {}", e)))?;

        Ok(aura_core::threshold::ThresholdSignature::single_signer(
            signature,
            public_key_package,
            current_epoch,
        ))
    }

    async fn threshold_config(
        &self,
        authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdConfig> {
        // Get current epoch for this authority
        let current_epoch = self.get_current_epoch(authority).await;

        // Retrieve stored threshold metadata for this epoch
        self.get_threshold_metadata(authority, current_epoch)
            .await
            .map(|metadata| aura_core::threshold::ThresholdConfig {
                threshold: metadata.threshold,
                total_participants: metadata.total_participants,
            })
    }

    async fn threshold_state(
        &self,
        authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdState> {
        // Get current epoch for this authority
        let current_epoch = self.get_current_epoch(authority).await;

        // Retrieve stored threshold metadata for this epoch
        self.get_threshold_metadata(authority, current_epoch)
            .await
            .map(|metadata| aura_core::threshold::ThresholdState {
                epoch: metadata.epoch,
                threshold: metadata.threshold,
                total_participants: metadata.total_participants,
                participants: metadata.resolved_participants(),
                agreement_mode: metadata.agreement_mode,
            })
    }

    async fn has_signing_capability(&self, authority: &AuthorityId) -> bool {
        let current_epoch = self.get_current_epoch(authority).await;
        let location = SecureStorageLocation::with_sub_key(
            "frost_keys",
            format!("{}/{}", authority, current_epoch),
            "1",
        );
        self.crypto.secure_storage()
            .secure_exists(&location)
            .await
            .unwrap_or(false)
    }

    async fn public_key_package(&self, authority: &AuthorityId) -> Option<Vec<u8>> {
        let location = SecureStorageLocation::new("frost_public_keys", format!("{}/0", authority));
        let caps = vec![SecureStorageCapability::Read];
        self.crypto.secure_storage()
            .secure_retrieve(&location, &caps)
            .await
            .ok()
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
            self.crypto.handler()
                .frost_rotate_keys(&[], 0, new_threshold, new_total_participants)
                .await?
        } else {
            let result = self
                .crypto
                .handler()
                .generate_signing_keys(new_threshold, new_total_participants)
                .await?;
            FrostKeyGenResult {
                key_packages: result.key_packages,
                public_key_package: result.public_key_package,
            }
        };

        // Store guardian key packages
        let caps = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        for (participant, key_package) in participants.iter().zip(key_result.key_packages.iter()) {
            let location = SecureStorageLocation::with_sub_key(
                "participant_shares",
                format!("{}/{}", authority, new_epoch),
                participant.storage_key(),
            );
            self.crypto.secure_storage()
                .secure_store(&location, key_package, &caps)
                .await?;
        }

        // Store public key package
        let pub_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", new_epoch),
        );
        self.crypto.secure_storage()
            .secure_store(&pub_location, &key_result.public_key_package, &caps)
            .await?;

        // Store threshold metadata for the new epoch
        self.store_threshold_metadata(
            authority,
            new_epoch,
            new_threshold,
            new_total_participants,
            participants,
            aura_core::threshold::AgreementMode::CoordinatorSoftSafe,
        )
        .await?;

        Ok((
            new_epoch,
            key_result.key_packages,
            key_result.public_key_package,
        ))
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
