//! L8 sentinels for actual custom runtime provider dispatch and outage retention.
use super::MemoryStorageHandler;
use async_trait::async_trait;
use aura_core::effects::crypto::{
    CryptoError, FrostKeyGenResult, FrostPublicCommitment, FrostSigningPackage,
    KeyDerivationContext, KeyGenerationMethod, SigningKeyGenResult, SigningMode,
};
use aura_core::effects::transport::{TransportEnvelope, TransportError, TransportStats};
use aura_core::effects::{
    ConsoleEffects, RandomCoreEffects, StorageCoreEffects, StorageError, StorageExtendedEffects,
    StorageStats, TransportEffects,
};
use aura_core::effects::{CryptoCoreEffects, CryptoExtendedEffects};
use aura_core::{AuraError, AuthorityId, ContextId};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// Concrete outage, retained by the actual selected provider.
#[derive(Debug, thiserror::Error)]
#[error("custom fixture provider unavailable")]
pub struct CustomProviderOutage;

/// Shared configured-provider sentinel; mocks and mutable fault controls stay in L8.
#[derive(Default)]
pub struct CustomProviderProbe {
    storage: MemoryStorageHandler,
    fault: AtomicBool,
    random_draws: AtomicUsize,
    console_calls: AtomicUsize,
    sends: AtomicUsize,
    ready: AtomicBool,
    inbound: async_lock::Mutex<VecDeque<TransportEnvelope>>,
    receives: AtomicUsize,
}
impl CustomProviderProbe {
    /// Change actual provider availability after runtime construction.
    pub fn set_fault(&self, fault: bool) {
        self.fault.store(fault, Ordering::SeqCst);
    }
    /// Expose a real configured channel for stable dispatch selection.
    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }
    /// Count calls to the supplied random owner, including receipt initialization.
    pub fn random_draws(&self) -> usize {
        self.random_draws.load(Ordering::SeqCst)
    }
    /// Count calls to the supplied console owner.
    pub fn console_calls(&self) -> usize {
        self.console_calls.load(Ordering::SeqCst)
    }
    /// Count attempts through the supplied transport owner.
    pub fn sends(&self) -> usize {
        self.sends.load(Ordering::SeqCst)
    }
    /// Enqueue a physical provider frame; runtime ownership is exercised by actual receive APIs.
    pub async fn push_inbound(&self, envelope: TransportEnvelope) {
        self.inbound.lock().await.push_back(envelope);
    }
    /// Count actual physical receive attempts independently of retained runtime reads.
    pub fn receives(&self) -> usize {
        self.receives.load(Ordering::SeqCst)
    }
    /// Inspect the actual underlying storage bytes to prove at-rest wrapping.
    pub async fn stored_bytes(&self) -> HashMap<String, Vec<u8>> {
        self.storage.get_all_data().await
    }
    fn required(&self) -> Result<(), AuraError> {
        if self.fault.load(Ordering::SeqCst) {
            return Err(AuraError::Storage {
                message: "selected provider outage".into(),
                source: Some(Arc::new(CustomProviderOutage)),
            });
        }
        Ok(())
    }
    fn storage_required(&self) -> Result<(), StorageError> {
        self.required()
            .map_err(|source| StorageError::BackendFailure {
                operation: "configured fixture storage".into(),
                source,
            })
    }
}
#[async_trait]
impl RandomCoreEffects for CustomProviderProbe {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.random_draws.fetch_add(1, Ordering::SeqCst);
        vec![0x93; len]
    }
    async fn random_bytes_32(&self) -> [u8; 32] {
        self.random_draws.fetch_add(1, Ordering::SeqCst);
        [0x93; 32]
    }
    async fn random_u64(&self) -> u64 {
        self.random_draws.fetch_add(1, Ordering::SeqCst);
        0x9393_9393_9393_9393
    }
}
#[async_trait]
impl ConsoleEffects for CustomProviderProbe {
    async fn log_info(&self, _: &str) -> Result<(), AuraError> {
        self.console_calls.fetch_add(1, Ordering::SeqCst);
        self.required()
    }
    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        self.log_info(message).await
    }
    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        self.log_info(message).await
    }
    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        self.log_info(message).await
    }
}
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl StorageCoreEffects for CustomProviderProbe {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        self.storage_required()?;
        self.storage.store(key, value).await
    }
    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        self.storage_required()?;
        self.storage.retrieve(key).await
    }
    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        self.storage_required()?;
        self.storage.remove(key).await
    }
    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        self.storage_required()?;
        self.storage.list_keys(prefix).await
    }
}
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl StorageExtendedEffects for CustomProviderProbe {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        self.storage_required()?;
        self.storage.exists(key).await
    }
    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        self.storage_required()?;
        self.storage.store_batch(pairs).await
    }
    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        self.storage_required()?;
        self.storage.retrieve_batch(keys).await
    }
    async fn clear_all(&self) -> Result<(), StorageError> {
        self.storage_required()?;
        self.storage.clear_all().await
    }
    async fn stats(&self) -> Result<StorageStats, StorageError> {
        self.storage_required()?;
        self.storage.stats().await
    }
}
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl TransportEffects for CustomProviderProbe {
    async fn send_envelope(&self, envelope: TransportEnvelope) -> Result<(), TransportError> {
        self.sends.fetch_add(1, Ordering::SeqCst);
        if self.fault.load(Ordering::SeqCst) {
            return Err(TransportError::DestinationUnreachable {
                destination: envelope.destination,
            });
        }
        Ok(())
    }
    async fn receive_envelope(&self) -> Result<TransportEnvelope, TransportError> {
        self.receives.fetch_add(1, Ordering::SeqCst);
        if self.fault.load(Ordering::SeqCst) {
            return Err(TransportError::ProtocolError {
                details: "configured fixture ingress fault".into(),
            });
        }
        self.inbound
            .lock()
            .await
            .pop_front()
            .ok_or(TransportError::NoMessage)
    }
    async fn receive_envelope_from(
        &self,
        source: AuthorityId,
        context: ContextId,
    ) -> Result<TransportEnvelope, TransportError> {
        self.receives.fetch_add(1, Ordering::SeqCst);
        if self.fault.load(Ordering::SeqCst) {
            return Err(TransportError::ProtocolError {
                details: "configured fixture ingress fault".into(),
            });
        }
        let mut inbound = self.inbound.lock().await;
        let index = inbound
            .iter()
            .position(|envelope| envelope.source == source && envelope.context == context)
            .ok_or(TransportError::NoMessage)?;
        inbound.remove(index).ok_or(TransportError::NoMessage)
    }
    async fn is_channel_established(&self, _: ContextId, _: AuthorityId) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
    async fn get_transport_stats(&self) -> TransportStats {
        TransportStats {
            active_channels: u32::from(self.ready.load(Ordering::SeqCst)),
            ..TransportStats::default()
        }
    }
}

/// Independently selected native provider fault for required Ed25519 tests.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[repr(usize)]
pub enum CustomEd25519Fault {
    /// Reject the selected provider's actual key generation call.
    #[error("configured Ed25519 generation unavailable")]
    Generate = 1,
    /// Reject the selected provider's actual signing call.
    #[error("configured Ed25519 signing unavailable")]
    Sign = 2,
    /// Reject the selected provider's actual verification call.
    #[error("configured Ed25519 verification unavailable")]
    Verify = 3,
}

/// Real crypto delegate with independently configured KDF and Ed25519 outages.
pub struct CustomCryptoProbe {
    inner: aura_effects::crypto::RealCryptoHandler,
    fault: AtomicBool,
    ed25519_fault: AtomicUsize,
}
impl Default for CustomCryptoProbe {
    fn default() -> Self {
        Self {
            inner: aura_effects::crypto::RealCryptoHandler::new(),
            fault: AtomicBool::new(false),
            ed25519_fault: AtomicUsize::new(0),
        }
    }
}
impl CustomCryptoProbe {
    /// Fault the actual configured crypto provider after construction.
    pub fn set_fault(&self, fault: bool) {
        self.fault.store(fault, Ordering::SeqCst);
    }
    /// Fault one selected Ed25519 operation without replacing the provider.
    pub fn set_ed25519_fault(&self, fault: Option<CustomEd25519Fault>) {
        self.ed25519_fault
            .store(fault.map_or(0, |fault| fault as usize), Ordering::SeqCst);
    }
    fn require_ed25519(&self, operation: CustomEd25519Fault) -> Result<(), CryptoError> {
        if self.ed25519_fault.load(Ordering::SeqCst) == operation as usize {
            return Err(AuraError::Crypto {
                message: "required configured Ed25519 operation failed".into(),
                source: Some(Arc::new(operation)),
            });
        }
        Ok(())
    }
}
#[async_trait]
impl RandomCoreEffects for CustomCryptoProbe {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.inner.random_bytes(len).await
    }
    async fn random_bytes_32(&self) -> [u8; 32] {
        self.inner.random_bytes_32().await
    }
    async fn random_u64(&self) -> u64 {
        self.inner.random_u64().await
    }
}
#[async_trait]
impl CryptoCoreEffects for CustomCryptoProbe {
    async fn kdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: u32,
    ) -> Result<Vec<u8>, CryptoError> {
        if self.fault.load(Ordering::SeqCst) {
            return Err(AuraError::Crypto {
                message: "configured KDF outage".into(),
                source: Some(Arc::new(CustomProviderOutage)),
            });
        }
        self.inner.kdf_derive(ikm, salt, info, output_len).await
    }
    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner.derive_key(master_key, context).await
    }
    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        self.require_ed25519(CustomEd25519Fault::Generate)?;
        self.inner.ed25519_generate_keypair().await
    }
    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.require_ed25519(CustomEd25519Fault::Sign)?;
        self.inner.ed25519_sign(message, private_key).await
    }
    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.require_ed25519(CustomEd25519Fault::Verify)?;
        self.inner
            .ed25519_verify(message, signature, public_key)
            .await
    }
    fn is_simulated(&self) -> bool {
        self.inner.is_simulated()
    }
    fn crypto_capabilities(&self) -> Vec<String> {
        let mut capabilities = self.inner.crypto_capabilities();
        capabilities.push("configured-custom-probe".into());
        capabilities
    }
    fn constant_time_eq(&self, left: &[u8], right: &[u8]) -> bool {
        self.inner.constant_time_eq(left, right)
    }
    fn secure_zero(&self, data: &mut [u8]) {
        self.inner.secure_zero(data);
    }
}
#[async_trait]
impl CryptoExtendedEffects for CustomCryptoProbe {
    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.inner
            .generate_signing_keys(threshold, max_signers)
            .await
    }
    async fn generate_signing_keys_with(
        &self,
        method: KeyGenerationMethod,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.inner
            .generate_signing_keys_with(method, threshold, max_signers)
            .await
    }
    async fn sign_participant_key_proof(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner
            .sign_participant_key_proof(message, key_package, mode)
            .await
    }
    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner.sign_with_key(message, key_package, mode).await
    }
    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        self.inner
            .verify_signature(message, signature, public_key_package, mode)
            .await
    }
    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        self.inner.frost_generate_keys(threshold, max_signers).await
    }
    async fn frost_generate_nonces(
        &self,
        key_package: &[u8],
    ) -> Result<aura_core::effects::crypto::FrostNonces, CryptoError> {
        self.inner.frost_generate_nonces(key_package).await
    }
    async fn frost_create_public_signing_package(
        &self,
        message: &[u8],
        commitments: &[FrostPublicCommitment],
        public_key_package: &[u8],
        threshold: u16,
    ) -> Result<FrostSigningPackage, CryptoError> {
        self.inner
            .frost_create_public_signing_package(
                message,
                commitments,
                public_key_package,
                threshold,
            )
            .await
    }
    async fn frost_sign_share_for_message(
        &self,
        package: &FrostSigningPackage,
        local_key_share: &[u8],
        nonces: aura_core::effects::crypto::RetiredFrostNonces,
        expected_message: &[u8],
        expected_public_key_package: &[u8],
        expected_threshold: u16,
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner
            .frost_sign_share_for_message(
                package,
                local_key_share,
                nonces,
                expected_message,
                expected_public_key_package,
                expected_threshold,
            )
            .await
    }
    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner
            .frost_aggregate_signatures(signing_package, signature_shares)
            .await
    }
    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.inner
            .frost_verify(message, signature, group_public_key)
            .await
    }
    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.inner.ed25519_public_key(private_key).await
    }
    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner.chacha20_encrypt(plaintext, key, nonce).await
    }
    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner.chacha20_decrypt(ciphertext, key, nonce).await
    }
    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner.aes_gcm_encrypt(plaintext, key, nonce).await
    }
    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner.aes_gcm_decrypt(ciphertext, key, nonce).await
    }
    async fn aes_gcm_encrypt_with_aad(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner
            .aes_gcm_encrypt_with_aad(plaintext, key, nonce, aad)
            .await
    }
    async fn aes_gcm_decrypt_with_aad(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner
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
        self.inner
            .frost_rotate_keys(old_shares, old_threshold, new_threshold, new_max_signers)
            .await
    }
    async fn convert_ed25519_to_x25519_public(
        &self,
        ed25519_public_key: &[u8],
    ) -> Result<[u8; 32], CryptoError> {
        self.inner
            .convert_ed25519_to_x25519_public(ed25519_public_key)
            .await
    }
    async fn convert_ed25519_to_x25519_private(
        &self,
        ed25519_private_key: &[u8],
    ) -> Result<[u8; 32], CryptoError> {
        self.inner
            .convert_ed25519_to_x25519_private(ed25519_private_key)
            .await
    }
}
