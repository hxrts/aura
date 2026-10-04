//! Explicit required verification-outage fixture owned by L8 test infrastructure.
use async_trait::async_trait;
use aura_core::effects::crypto::{CryptoError, KeyDerivationContext, SigningMode};
use aura_core::effects::{CryptoCoreEffects, CryptoExtendedEffects, RandomCoreEffects};
use std::sync::Arc;

/// Delegates core primitives while failing unified verification with the exact
/// injected required-provider cause. It grants no signing or runtime authority.
pub struct VerificationFailureFixture<T> {
    inner: T,
    source: Arc<dyn std::error::Error + Send + Sync>,
}
impl<T> VerificationFailureFixture<T> {
    /// Retain the concrete injected failure; no diagnostic string substitutes
    /// for the original native provider source.
    pub fn new(inner: T, source: Arc<dyn std::error::Error + Send + Sync>) -> Self {
        Self { inner, source }
    }
}
#[async_trait]
impl<T: RandomCoreEffects> RandomCoreEffects for VerificationFailureFixture<T> {
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
impl<T: CryptoCoreEffects> CryptoCoreEffects for VerificationFailureFixture<T> {
    async fn kdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        len: u32,
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner.kdf_derive(ikm, salt, info, len).await
    }
    async fn derive_key(
        &self,
        master: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        self.inner.derive_key(master, context).await
    }
    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        self.inner.ed25519_generate_keypair().await
    }
    async fn ed25519_sign(&self, message: &[u8], key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.inner.ed25519_sign(message, key).await
    }
    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.inner.ed25519_verify(message, signature, key).await
    }
    fn is_simulated(&self) -> bool {
        self.inner.is_simulated()
    }
    fn crypto_capabilities(&self) -> Vec<String> {
        self.inner.crypto_capabilities()
    }
    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.inner.constant_time_eq(a, b)
    }
    fn secure_zero(&self, data: &mut [u8]) {
        self.inner.secure_zero(data);
    }
}
#[async_trait]
impl<T: CryptoCoreEffects> CryptoExtendedEffects for VerificationFailureFixture<T> {
    async fn verify_signature(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _package: &[u8],
        _mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        Err(aura_core::AuraError::crypto_with_source(
            "required injected signature provider failure",
            self.source.clone(),
        ))
    }
}
