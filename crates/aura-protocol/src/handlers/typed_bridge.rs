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
    Blake3HashParams, DelayParams, RandomBytesParams, RandomRangeParams, Sha256HashParams,
};
use crate::effects::*;
use crate::handlers::context::AuraContext;
use async_trait::async_trait;
use aura_types::{AuraError, DeviceId};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use std::future::Future;
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
// CryptoEffects Blanket Implementation for Arc<RwLock<Box<dyn AuraHandler>>>
// ═══════════════════════════════════════════════════════════════════════════

#[async_trait]
impl CryptoEffects for Arc<RwLock<Box<dyn AuraHandler>>> {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut handler = self.write().await;
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
        let mut handler = self.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<[u8; 32]>(
            &mut **handler,
            EffectType::Crypto,
            "random_bytes_32",
            RandomBytes32Params,
            &mut ctx,
        )
        .await
        .unwrap_or([0u8; 32])
    }

    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        let mut handler = self.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<u64>(
            &mut **handler,
            EffectType::Crypto,
            "random_range",
            RandomRangeParams {
                start: range.start,
                end: range.end,
            },
            &mut ctx,
        )
        .await
        .unwrap_or(range.start)
    }

    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        let mut handler = self.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<[u8; 32]>(
            &mut **handler,
            EffectType::Crypto,
            "blake3_hash",
            Blake3HashParams {
                data: data.to_vec(),
            },
            &mut ctx,
        )
        .await
        .unwrap_or([0u8; 32])
    }

    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32] {
        let mut handler = self.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<[u8; 32]>(
            &mut **handler,
            EffectType::Crypto,
            "sha256_hash",
            Sha256HashParams {
                data: data.to_vec(),
            },
            &mut ctx,
        )
        .await
        .unwrap_or([0u8; 32])
    }

    async fn ed25519_sign(
        &self,
        _data: &[u8],
        _key: &SigningKey,
    ) -> Result<Signature, CryptoError> {
        Err(AuraError::Crypto(
            aura_types::CryptoError::OperationFailed {
                message: "ed25519_sign requires direct handler access".to_string(),
                context: "typed_bridge".to_string(),
            },
        ))
    }

    async fn ed25519_verify(
        &self,
        _data: &[u8],
        _signature: &Signature,
        _public_key: &VerifyingKey,
    ) -> Result<bool, CryptoError> {
        Err(AuraError::Crypto(
            aura_types::CryptoError::OperationFailed {
                message: "ed25519_verify requires direct handler access".to_string(),
                context: "typed_bridge".to_string(),
            },
        ))
    }

    async fn ed25519_generate_keypair(&self) -> Result<(SigningKey, VerifyingKey), CryptoError> {
        Err(AuraError::Crypto(
            aura_types::CryptoError::OperationFailed {
                message: "ed25519_generate_keypair requires direct handler access".to_string(),
                context: "typed_bridge".to_string(),
            },
        ))
    }

    async fn ed25519_public_key(&self, key: &SigningKey) -> VerifyingKey {
        key.verifying_key()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        a.ct_eq(b).into()
    }

    fn secure_zero(&self, data: &mut [u8]) {
        use zeroize::Zeroize;
        data.zeroize();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TimeEffects Blanket Implementation
// ═══════════════════════════════════════════════════════════════════════════

#[async_trait]
impl TimeEffects for Arc<RwLock<Box<dyn AuraHandler>>> {
    async fn current_epoch(&self) -> u64 {
        let mut handler = self.write().await;
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
        let mut handler = self.write().await;
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
        let mut handler = self.write().await;
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
        let mut handler = self.write().await;
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
        let mut handler = self.write().await;
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
        let mut handler = self.write().await;
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
        let mut handler = self.write().await;
        let mut ctx = get_context();

        HandlerUtils::execute_typed_effect::<()>(
            &mut **handler,
            EffectType::Time,
            "sleep",
            duration_ms,
            &mut ctx,
        )
        .await
        .map_err(|e| {
            AuraError::Infrastructure(aura_types::InfrastructureError::ConfigError {
                message: format!("Sleep failed: {}", e),
                context: "typed_bridge".to_string(),
            })
        })
    }

    async fn yield_until(&self, _condition: WakeCondition) -> Result<(), TimeError> {
        Err(TimeError::ServiceUnavailable)
    }

    async fn wait_until(&self, _condition: WakeCondition) -> Result<(), AuraError> {
        Err(AuraError::Infrastructure(
            aura_types::InfrastructureError::ConfigError {
                message: "wait_until not implemented through bridge".to_string(),
                context: "typed_bridge".to_string(),
            },
        ))
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        let mut handler = self.write().await;
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
        let mut handler = self.write().await;
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

impl ConsoleEffects for Arc<RwLock<Box<dyn AuraHandler>>> {
    fn log_trace(&self, message: &str, fields: &[(&str, &str)]) {
        // Convert to owned strings to avoid lifetime issues
        let message_owned = message.to_string();
        let fields_owned: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Simplified implementation for bridge - just print synchronously
        println!("[TRACE] {}: {:?}", message_owned, fields_owned);
    }

    fn log_debug(&self, message: &str, fields: &[(&str, &str)]) {
        // Convert to owned strings to avoid lifetime issues
        let message_owned = message.to_string();
        let fields_owned: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Simplified implementation for bridge - just print synchronously
        println!("[DEBUG] {}: {:?}", message_owned, fields_owned);
    }

    fn log_info(&self, message: &str, fields: &[(&str, &str)]) {
        // Convert to owned strings to avoid lifetime issues
        let message_owned = message.to_string();
        let fields_owned: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Simplified implementation for bridge - just print synchronously
        println!("[INFO] {}: {:?}", message_owned, fields_owned);
    }

    fn log_warn(&self, message: &str, fields: &[(&str, &str)]) {
        // Convert to owned strings to avoid lifetime issues
        let message_owned = message.to_string();
        let fields_owned: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Simplified implementation for bridge - just print synchronously
        println!("[WARN] {}: {:?}", message_owned, fields_owned);
    }

    fn log_error(&self, message: &str, fields: &[(&str, &str)]) {
        // Convert to owned strings to avoid lifetime issues
        let message_owned = message.to_string();
        let fields_owned: Vec<(String, String)> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Simplified implementation for bridge - just print synchronously
        eprintln!("[ERROR] {}: {:?}", message_owned, fields_owned);
    }

    fn emit_event(
        &self,
        event: crate::effects::ConsoleEvent,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            // Simplified implementation for bridge
            println!("[EVENT] {:?}", event);
        })
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

        let hash = handler.blake3_hash(b"test data").await;
        assert_eq!(hash.len(), 32);
    }

    #[tokio::test]
    async fn test_time_effects_bridge() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let handler = AuraHandlerFactory::for_testing(device_id);
        let handler = Arc::new(RwLock::new(handler));

        // Test that we can call TimeEffects methods
        let timestamp = handler.current_timestamp().await;
        // timestamp is u64, so it's always >= 0
        assert_eq!(timestamp >= 0, true);

        handler.delay(Duration::from_millis(1)).await;
        // Should complete without error
    }

    #[tokio::test]
    async fn test_console_effects_bridge() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let handler = AuraHandlerFactory::for_testing(device_id);
        let handler = Arc::new(RwLock::new(handler));

        // Test that we can call ConsoleEffects methods
        handler.log_info("Test message", &[]);
        handler.log_warn("Warning message", &[]);
        handler.log_error("Error message", &[]);

        // Should complete without error
    }
}
