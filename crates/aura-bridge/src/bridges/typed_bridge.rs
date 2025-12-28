//! Typed bridge for type-erased handlers
//!
//! This module provides implementations that allow type-erased `AuraHandler` to be
//! used through typed effect trait interfaces.
//!
//! # Design Note
//!
//! Due to the `&mut self` requirement of `AuraHandler::execute_effect`, the blanket
//! implementations use `Arc<RwLock<Box<dyn AuraHandler>>>` to provide interior mutability.
//!
//! # Usage
//!
//! ```ignore
//! use std::sync::Arc;
//! use async_lock::RwLock;
//!
//! // Wrap handler for typed trait usage
//! let handler: Arc<RwLock<Box<dyn AuraHandler>>> = Arc::new(RwLock::new(
//!     AuraHandlerFactory::for_testing(device_id).unwrap()
//! ));
//! let ctx = AuraContext::for_testing(device_id);
//! let bridge = TypedHandlerBridge::new(handler, ctx);
//!
//! // Now can use typed traits
//! let bytes = bridge.random_bytes(32).await;
//! ```

use crate::bridges::config::BridgeRuntimeConfig;
use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::crypto::{FrostKeyGenResult, SigningKeyGenResult};
use aura_core::effects::random::RandomCoreEffects;
use aura_core::effects::{CryptoCoreEffects, CryptoError, CryptoExtendedEffects};
use aura_core::AuraError;
use aura_protocol::effects::params::RandomBytesParams;
use aura_protocol::handlers::{AuraContext, AuraHandler, EffectType, HandlerUtils};
use std::sync::Arc;

// ═══════════════════════════════════════════════════════════════════════════
// Newtype Wrapper to Avoid Orphan Rules
// ═══════════════════════════════════════════════════════════════════════════

/// Newtype wrapper around Arc<RwLock<Box<dyn AuraHandler>>> to enable trait implementations
/// without violating orphan rules.
pub struct TypedHandlerBridge {
    handler: Arc<RwLock<Box<dyn AuraHandler>>>,
    context: Arc<RwLock<AuraContext>>,
    config: BridgeRuntimeConfig,
}

impl TypedHandlerBridge {
    /// Create a new typed handler bridge
    pub fn new(handler: Arc<RwLock<Box<dyn AuraHandler>>>, context: AuraContext) -> Self {
        Self {
            handler,
            context: Arc::new(RwLock::new(context)),
            config: BridgeRuntimeConfig::default(),
        }
    }

    /// Create a new typed handler bridge with explicit config.
    pub fn new_with_config(
        handler: Arc<RwLock<Box<dyn AuraHandler>>>,
        context: AuraContext,
        config: BridgeRuntimeConfig,
    ) -> Self {
        Self {
            handler,
            context: Arc::new(RwLock::new(context)),
            config,
        }
    }

    /// Get a reference to the underlying handler
    pub fn inner(&self) -> &Arc<RwLock<Box<dyn AuraHandler>>> {
        &self.handler
    }

    /// Replace the active context for subsequent effect executions.
    pub async fn set_context(&self, context: AuraContext) {
        let mut ctx = self.context.write().await;
        *ctx = context;
    }

    async fn get_context(&self) -> AuraContext {
        self.context.read().await.clone()
    }

    fn handle_error<T: Default>(&self, err: AuraError) -> T {
        if self.config.panic_on_error {
            panic!("TypedHandlerBridge error: {err}");
        }
        T::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// RandomCoreEffects Implementation for TypedHandlerBridge
// ═══════════════════════════════════════════════════════════════════════════

#[async_trait]
impl RandomCoreEffects for TypedHandlerBridge {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut handler = self.handler.write().await;
        let ctx = self.get_context().await;

        HandlerUtils::execute_typed_effect::<Vec<u8>>(
            &mut **handler,
            EffectType::Random,
            "random_bytes",
            RandomBytesParams { len },
            &ctx,
        )
        .await
        .unwrap_or_else(|err| self.handle_error(err.into()))
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut handler = self.handler.write().await;
        let ctx = self.get_context().await;

        HandlerUtils::execute_typed_effect::<[u8; 32]>(
            &mut **handler,
            EffectType::Random,
            "random_bytes_32",
            RandomBytesParams { len: 32 },
            &ctx,
        )
        .await
        .unwrap_or_else(|err| self.handle_error(err.into()))
    }

    async fn random_u64(&self) -> u64 {
        let mut handler = self.handler.write().await;
        let ctx = self.get_context().await;

        HandlerUtils::execute_typed_effect::<u64>(
            &mut **handler,
            EffectType::Random,
            "random_u64",
            (),
            &ctx,
        )
        .await
        .unwrap_or_else(|err| self.handle_error(err.into()))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CryptoEffects Implementation for TypedHandlerBridge
// ═══════════════════════════════════════════════════════════════════════════

#[async_trait]
impl CryptoCoreEffects for TypedHandlerBridge {
    // Note: hash is NOT an algebraic effect - use aura_core::hash::hash() instead

    async fn ed25519_sign(&self, data: &[u8], private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut handler = self.handler.write().await;
        let ctx = self.get_context().await;

        let params = (data.to_vec(), private_key.to_vec());

        HandlerUtils::execute_typed_effect::<Result<Vec<u8>, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "ed25519_sign",
            &params,
            &ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("ed25519_sign bridge failed")))
    }

    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        let mut handler = self.handler.write().await;
        let ctx = self.get_context().await;

        let params = (data.to_vec(), signature.to_vec(), public_key.to_vec());

        HandlerUtils::execute_typed_effect::<Result<bool, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "ed25519_verify",
            &params,
            &ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("ed25519_verify bridge failed")))
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let mut handler = self.handler.write().await;
        let ctx = self.get_context().await;

        HandlerUtils::execute_typed_effect::<Result<(Vec<u8>, Vec<u8>), CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "ed25519_generate_keypair",
            (),
            &ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("ed25519_generate_keypair bridge failed")))
    }

    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        let mut handler = self.handler.write().await;
        let ctx = self.get_context().await;

        let params = (ikm.to_vec(), salt.to_vec(), info.to_vec(), output_len);

        HandlerUtils::execute_typed_effect::<Result<Vec<u8>, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "hkdf_derive",
            &params,
            &ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("hkdf_derive bridge failed")))
    }

    // Note: hmac is NOT an algebraic effect - use aura_core::hash::hash() instead

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &aura_core::effects::crypto::KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        let mut handler = self.handler.write().await;
        let ctx = self.get_context().await;

        let params = (master_key.to_vec(), context.clone());

        HandlerUtils::execute_typed_effect::<Result<Vec<u8>, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "derive_key",
            &params,
            &ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("derive_key bridge failed")))
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        a.ct_eq(b).into()
    }

    fn secure_zero(&self, data: &mut [u8]) {
        use zeroize::Zeroize;
        data.zeroize();
    }

    fn is_simulated(&self) -> bool {
        false // Bridge implementations assume production mode
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "bridge_hash".to_string(),
            "bridge_sha256".to_string(),
            "bridge_hkdf".to_string(),
        ]
    }
}

#[async_trait]
impl CryptoExtendedEffects for TypedHandlerBridge {
    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut handler = self.handler.write().await;
        let ctx = self.get_context().await;

        HandlerUtils::execute_typed_effect::<Result<Vec<u8>, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "ed25519_public_key",
            private_key.to_vec(),
            &ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("ed25519_public_key bridge failed")))
    }

    async fn frost_generate_keys(
        &self,
        _threshold: u16,
        _max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        Err(AuraError::crypto(
            "FROST operations not supported through bridge",
        ))
    }

    async fn frost_generate_nonces(&self, _key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        Err(AuraError::crypto(
            "FROST operations not supported through bridge",
        ))
    }

    async fn frost_create_signing_package(
        &self,
        _message: &[u8],
        _nonces: &[Vec<u8>],
        _participants: &[u16],
        _public_key_package: &[u8],
    ) -> Result<aura_core::effects::crypto::FrostSigningPackage, CryptoError> {
        Err(AuraError::crypto(
            "FROST operations not supported through bridge",
        ))
    }

    async fn frost_sign_share(
        &self,
        _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        _key_share: &[u8],
        _nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(AuraError::crypto(
            "FROST operations not supported through bridge",
        ))
    }

    async fn frost_aggregate_signatures(
        &self,
        _signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        _signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(AuraError::crypto(
            "FROST operations not supported through bridge",
        ))
    }

    async fn frost_verify(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        Err(AuraError::crypto(
            "FROST operations not supported through bridge",
        ))
    }

    async fn chacha20_encrypt(
        &self,
        _plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(AuraError::crypto(
            "Symmetric encryption not supported through bridge",
        ))
    }

    async fn chacha20_decrypt(
        &self,
        _ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(AuraError::crypto(
            "Symmetric decryption not supported through bridge",
        ))
    }

    async fn aes_gcm_encrypt(
        &self,
        _plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(AuraError::crypto(
            "AES-GCM encryption not supported through bridge",
        ))
    }

    async fn aes_gcm_decrypt(
        &self,
        _ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        Err(AuraError::crypto(
            "AES-GCM decryption not supported through bridge",
        ))
    }

    async fn frost_rotate_keys(
        &self,
        _old_shares: &[Vec<u8>],
        _old_threshold: u16,
        _new_threshold: u16,
        _new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        Err(AuraError::crypto(
            "FROST key rotation not supported through bridge",
        ))
    }

    async fn generate_signing_keys(
        &self,
        _threshold: u16,
        _max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        Err(AuraError::crypto(
            "Signing key generation not supported through bridge",
        ))
    }

    async fn sign_with_key(
        &self,
        _message: &[u8],
        _key_package: &[u8],
        _mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        Err(AuraError::crypto(
            "sign_with_key not supported through bridge",
        ))
    }

    async fn verify_signature(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _public_key_package: &[u8],
        _mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        Err(AuraError::crypto(
            "verify_signature not supported through bridge",
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TimeEffects Blanket Implementation
// ═══════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════
// ConsoleEffects Blanket Implementation
// ═══════════════════════════════════════════════════════════════════════════

#[async_trait]
impl aura_core::effects::ConsoleEffects for TypedHandlerBridge {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        println!("[INFO] {}", message);
        Ok(())
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        println!("[WARN] {}", message);
        Ok(())
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        eprintln!("[ERROR] {}", message);
        Ok(())
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        println!("[DEBUG] {}", message);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{DeviceId, ExecutionMode};
    use aura_protocol::handlers::core::erased::AuraHandlerFactory as ErasedAuraHandlerFactory;

    #[tokio::test]
    async fn test_crypto_effects_bridge() {
        let device_id = DeviceId::new_from_entropy([1u8; 32]);
        let handler = ErasedAuraHandlerFactory::for_testing(device_id);
        let ctx = aura_protocol::handlers::AuraContext::for_testing(device_id);

        // Test that we can call effects through the handler interface - only test supported effects
        if handler.supports_effect(EffectType::Random) {
            let param_bytes = serde_json::to_vec(&32_usize).unwrap();
            let result = handler
                .execute_effect(EffectType::Random, "random_bytes", &param_bytes, &ctx)
                .await;
            assert!(
                result.is_ok(),
                "random_bytes should be supported: {:?}",
                result.err()
            );
        }

        if handler.supports_effect(EffectType::Crypto) {
            let param_bytes = serde_json::to_vec(b"test data").unwrap();
            let result = handler
                .execute_effect(EffectType::Crypto, "hash_data", &param_bytes, &ctx)
                .await;
            assert!(
                result.is_ok(),
                "hash_data should be supported: {:?}",
                result.err()
            );
        }

        // At minimum, verify the handler interface works
        assert_eq!(handler.execution_mode(), ExecutionMode::Testing);
    }

    #[tokio::test]
    async fn test_time_effects_bridge() {
        let device_id = DeviceId::new_from_entropy([2u8; 32]);
        let handler = ErasedAuraHandlerFactory::for_testing(device_id);
        let _handler = Arc::new(RwLock::new(handler));

        // Test that we can create and wrap the handler
        // In practice, time effects would be called through the effect system
        // This just verifies the handler can be created and wrapped correctly
    }

    #[tokio::test]
    async fn test_console_effects_bridge() {
        let device_id = DeviceId::new_from_entropy([3u8; 32]);
        let handler = ErasedAuraHandlerFactory::for_testing(device_id);
        let _handler = Arc::new(RwLock::new(handler));

        // Test that we can create the handler and use it for basic operations
        // In practice, effects would be called through the effect system
        // This just verifies the handler can be created and wrapped correctly
    }
}
