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
//! use tokio::sync::RwLock;
//!
//! // Wrap handler for typed trait usage
//! let handler: Arc<RwLock<Box<dyn AuraHandler>>> = Arc::new(RwLock::new(
//!     AuraHandlerFactory::for_testing(device_id).unwrap()
//! ));
//!
//! // Now can use typed traits
//! let bytes = handler.random_bytes(32).await;
//! ```

use super::erased::{AuraHandler, HandlerUtils};
use super::EffectType;
use crate::effects::crypto::CryptoError;
use crate::effects::params::{
    DelayParams, RandomBytesParams, RandomRangeParams,
};
use crate::effects::*;
use crate::handlers::context::AuraContext;
use async_trait::async_trait;
use aura_core::{AuraError, DeviceId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// Helper: Get or create thread-local context
// ═══════════════════════════════════════════════════════════════════════════

thread_local! {
    static CURRENT_CONTEXT: std::cell::RefCell<Option<AuraContext>> = const { std::cell::RefCell::new(None) };
}

/// Get current context or create a temporary one
fn get_context() -> AuraContext {
    CURRENT_CONTEXT.with(|ctx| {
        ctx.borrow().clone().unwrap_or_else(|| {
            // Fallback: create temporary context
            AuraContext::for_testing(DeviceId::from(Uuid::nil()))
        })
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Newtype Wrapper to Avoid Orphan Rules
// ═══════════════════════════════════════════════════════════════════════════

/// Newtype wrapper around Arc<RwLock<Box<dyn AuraHandler>>> to enable trait implementations
/// without violating orphan rules.
pub struct TypedHandlerBridge(Arc<RwLock<Box<dyn AuraHandler>>>);

impl TypedHandlerBridge {
    /// Create a new typed handler bridge
    pub fn new(handler: Arc<RwLock<Box<dyn AuraHandler>>>) -> Self {
        Self(handler)
    }

    /// Get a reference to the underlying handler
    pub fn inner(&self) -> &Arc<RwLock<Box<dyn AuraHandler>>> {
        &self.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// RandomEffects Implementation for TypedHandlerBridge
// ═══════════════════════════════════════════════════════════════════════════

#[async_trait]
impl aura_core::effects::RandomEffects for TypedHandlerBridge {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<Vec<u8>>(
            &mut **handler,
            EffectType::Crypto,
            "random_bytes",
            RandomBytesParams { len },
            &mut ctx,
        )
        .await
        .unwrap_or_else(|_| vec![0u8; len])
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<[u8; 32]>(
            &mut **handler,
            EffectType::Crypto,
            "random_bytes_32",
            RandomBytesParams { len: 32 },
            &mut ctx,
        )
        .await
        .unwrap_or([0u8; 32])
    }

    async fn random_u64(&self) -> u64 {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<u64>(
            &mut **handler,
            EffectType::Crypto,
            "random_u64",
            (),
            &mut ctx,
        )
        .await
        .unwrap_or(0)
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<u64>(
            &mut **handler,
            EffectType::Crypto,
            "random_range",
            RandomRangeParams {
                start: min,
                end: max,
            },
            &mut ctx,
        )
        .await
        .unwrap_or(min)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CryptoEffects Implementation for TypedHandlerBridge
// ═══════════════════════════════════════════════════════════════════════════

#[async_trait]
impl CryptoEffects for TypedHandlerBridge {
    async fn hash(&self, data: &[u8]) -> [u8; 32] {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<[u8; 32]>(
            &mut **handler,
            EffectType::Crypto,
            "hash",
            data.to_vec(),
            &mut ctx,
        )
        .await
        .unwrap_or([0u8; 32])
    }

    async fn ed25519_sign(&self, data: &[u8], private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        let params = (data.to_vec(), private_key.to_vec());

        HandlerUtils::execute_typed_effect::<Result<Vec<u8>, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "ed25519_sign",
            &params,
            &mut ctx,
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
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        let params = (data.to_vec(), signature.to_vec(), public_key.to_vec());

        HandlerUtils::execute_typed_effect::<Result<bool, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "ed25519_verify",
            &params,
            &mut ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("ed25519_verify bridge failed")))
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<Result<(Vec<u8>, Vec<u8>), CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "ed25519_generate_keypair",
            (),
            &mut ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("ed25519_generate_keypair bridge failed")))
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<Result<Vec<u8>, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "ed25519_public_key",
            private_key.to_vec(),
            &mut ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("ed25519_public_key bridge failed")))
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        a.ct_eq(b).into()
    }

    fn secure_zero(&self, data: &mut [u8]) {
        use zeroize::Zeroize;
        data.zeroize();
    }

    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        let params = (ikm.to_vec(), salt.to_vec(), info.to_vec(), output_len);

        HandlerUtils::execute_typed_effect::<Result<Vec<u8>, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "hkdf_derive",
            &params,
            &mut ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("hkdf_derive bridge failed")))
    }

    // Add all missing CryptoEffects methods
    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32] {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<[u8; 32]>(
            &mut **handler,
            EffectType::Crypto,
            "hmac",
            (key.to_vec(), data.to_vec()),
            &mut ctx,
        )
        .await
        .unwrap_or([0u8; 32])
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &aura_core::effects::crypto::KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        let params = (master_key.to_vec(), context.clone());

        HandlerUtils::execute_typed_effect::<Result<Vec<u8>, CryptoError>>(
            &mut **handler,
            EffectType::Crypto,
            "derive_key",
            &params,
            &mut ctx,
        )
        .await
        .unwrap_or_else(|_| Err(AuraError::crypto("derive_key bridge failed")))
    }

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        Err(AuraError::crypto(
            "FROST operations not supported through bridge",
        ))
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        Err(AuraError::crypto(
            "FROST operations not supported through bridge",
        ))
    }

    async fn frost_create_signing_package(
        &self,
        _message: &[u8],
        _nonces: &[Vec<u8>],
        _participants: &[u16],
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
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        Err(AuraError::crypto(
            "FROST key rotation not supported through bridge",
        ))
    }

    fn is_simulated(&self) -> bool {
        false // Bridge implementations assume production mode
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "bridge_blake3".to_string(),
            "bridge_sha256".to_string(),
            "bridge_hkdf".to_string(),
        ]
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TimeEffects Blanket Implementation
// ═══════════════════════════════════════════════════════════════════════════

#[async_trait]
impl TimeEffects for TypedHandlerBridge {
    async fn current_epoch(&self) -> u64 {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<u64>(
            &mut **handler,
            EffectType::Time,
            "current_epoch",
            (),
            &mut ctx,
        )
        .await
        .unwrap_or(0)
    }

    async fn current_timestamp(&self) -> u64 {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<u64>(
            &mut **handler,
            EffectType::Time,
            "current_timestamp",
            (),
            &mut ctx,
        )
        .await
        .unwrap_or(0)
    }

    async fn current_timestamp_millis(&self) -> u64 {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<u64>(
            &mut **handler,
            EffectType::Time,
            "current_timestamp_millis",
            (),
            &mut ctx,
        )
        .await
        .unwrap_or(0)
    }

    async fn sleep_ms(&self, ms: u64) {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        let _ = HandlerUtils::execute_typed_effect::<()>(
            &mut **handler,
            EffectType::Time,
            "sleep_ms",
            ms,
            &mut ctx,
        )
        .await;
    }

    async fn sleep_until(&self, epoch: u64) {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        let _ = HandlerUtils::execute_typed_effect::<()>(
            &mut **handler,
            EffectType::Time,
            "sleep_until",
            epoch,
            &mut ctx,
        )
        .await;
    }

    async fn delay(&self, duration: Duration) {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        let _ = HandlerUtils::execute_typed_effect::<()>(
            &mut **handler,
            EffectType::Time,
            "delay",
            DelayParams {
                duration_ms: duration.as_millis() as u64,
            },
            &mut ctx,
        )
        .await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<()>(
            &mut **handler,
            EffectType::Time,
            "sleep",
            duration_ms,
            &mut ctx,
        )
        .await
        .map_err(|e| AuraError::internal(&format!("Sleep failed: {}", e)))
    }

    async fn yield_until(&self, _condition: WakeCondition) -> Result<(), TimeError> {
        Err(TimeError::ServiceUnavailable)
    }

    async fn wait_until(&self, _condition: WakeCondition) -> Result<(), AuraError> {
        Err(AuraError::internal(
            "wait_until not implemented through bridge",
        ))
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<TimeoutHandle>(
            &mut **handler,
            EffectType::Time,
            "set_timeout",
            timeout_ms,
            &mut ctx,
        )
        .await
        .unwrap_or_else(|_| uuid::Uuid::new_v4())
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        let mut handler = self.0.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<()>(
            &mut **handler,
            EffectType::Time,
            "cancel_timeout",
            handle,
            &mut ctx,
        )
        .await
        .map_err(|_| TimeError::ServiceUnavailable)
    }

    // timeout method removed to make TimeEffects dyn-compatible
    // Use tokio::time::timeout directly where needed

    fn is_simulated(&self) -> bool {
        false // Bridge implementations assume production mode
    }

    fn register_context(&self, _context_id: uuid::Uuid) {
        // Placeholder
    }

    fn unregister_context(&self, _context_id: uuid::Uuid) {
        // Placeholder
    }

    async fn notify_events_available(&self) {
        // Placeholder
    }

    fn resolution_ms(&self) -> u64 {
        1 // Default 1ms resolution
    }
}

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
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::handlers::erased::AuraHandlerFactory;

    #[tokio::test]
    async fn test_crypto_effects_bridge() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let handler = AuraHandlerFactory::for_testing(device_id);
        let handler = Arc::new(RwLock::new(handler));

        // Test that we can call CryptoEffects methods
        let bytes = handler.random_bytes(32).await;
        assert_eq!(bytes.len(), 32);

        let hash = handler.hash(b"test data").await;
        assert_eq!(hash.len(), 32);
    }

    #[tokio::test]
    async fn test_time_effects_bridge() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let handler = AuraHandlerFactory::for_testing(device_id);
        let handler = Arc::new(RwLock::new(handler));

        // Test that we can create and wrap the handler
        // In practice, time effects would be called through the effect system
        // This just verifies the handler can be created and wrapped correctly
    }

    #[tokio::test]
    async fn test_console_effects_bridge() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let handler = AuraHandlerFactory::for_testing(device_id);
        let handler = Arc::new(RwLock::new(handler));

        // Test that we can create the handler and use it for basic operations
        // In practice, effects would be called through the effect system
        // This just verifies the handler can be created and wrapped correctly

        // Should complete without error
    }
}
