//! Primary handler interface for the Aura architecture
//!
//! This module defines the core handler trait that all Aura handlers implement.
//! Uses type erasure to enable trait object compatibility while maintaining type safety.

use async_trait::async_trait;

use super::context::AuraContext;
use super::{AuraHandlerError, EffectType, ExecutionMode};
use aura_types::sessions::LocalSessionType;

/// Primary interface for all Aura handlers
///
/// This trait defines the unified interface for effect execution and session
/// interpretation. All handlers in the Aura system implement this trait.
/// Uses serialized bytes for parameters and results to enable trait object compatibility.
#[async_trait]
pub trait AuraHandler: Send + Sync {
    /// Execute an effect with serialized parameters and return serialized result
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError>;

    /// Execute a session type
    async fn execute_session(
        &mut self,
        session: LocalSessionType,
        ctx: &mut AuraContext,
    ) -> Result<(), AuraHandlerError>;

    /// Check if this handler supports a specific effect type
    fn supports_effect(&self, effect_type: EffectType) -> bool;

    /// Get the execution mode of this handler
    fn execution_mode(&self) -> ExecutionMode;

    /// Get supported effect types
    fn supported_effects(&self) -> Vec<EffectType> {
        EffectType::all()
            .into_iter()
            .filter(|&effect_type| self.supports_effect(effect_type))
            .collect()
    }
}

/// Factory for creating Aura handlers
///
/// Creates handlers that implement the unified AuraHandler trait.
pub struct AuraHandlerFactory;

impl AuraHandlerFactory {
    /// Create a handler for testing
    pub fn for_testing(device_id: aura_types::DeviceId) -> Box<dyn AuraHandler> {
        let handler = crate::handlers::CompositeHandler::for_testing(device_id.into());
        Box::new(handler)
    }

    /// Create a handler for production
    pub fn for_production(
        device_id: aura_types::DeviceId,
    ) -> Result<Box<dyn AuraHandler>, AuraHandlerError> {
        let handler = crate::handlers::CompositeHandler::for_production(device_id.into());
        Ok(Box::new(handler))
    }

    /// Create a handler for simulation
    pub fn for_simulation(
        device_id: aura_types::DeviceId,
        seed: u64,
    ) -> Box<dyn AuraHandler> {
        let handler = crate::handlers::CompositeHandler::for_simulation(device_id.into());
        Box::new(handler)
    }
}

/// Convenience type alias for boxed handlers
pub type BoxedHandler = Box<dyn AuraHandler>;

/// Utilities for working with Aura handlers
pub struct HandlerUtils;

impl HandlerUtils {
    /// Execute a typed effect through a handler
    pub async fn execute_typed_effect<T>(
        handler: &mut dyn AuraHandler,
        effect_type: EffectType,
        operation: &str,
        parameters: impl serde::Serialize,
        ctx: &mut AuraContext,
    ) -> Result<T, AuraHandlerError>
    where
        T: serde::de::DeserializeOwned + Send + Sync,
    {
        // Serialize parameters
        let param_bytes =
            bincode::serialize(&parameters).map_err(|e| AuraHandlerError::EffectSerialization {
                effect_type,
                operation: operation.to_string(),
                source: e.into(),
            })?;

        // Execute through handler interface
        let result_bytes = handler
            .execute_effect(effect_type, operation, &param_bytes, ctx)
            .await?;

        // Deserialize the result
        bincode::deserialize(&result_bytes).map_err(|e| AuraHandlerError::EffectDeserialization {
            effect_type,
            operation: operation.to_string(),
            source: e.into(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use aura_types::identifiers::DeviceId;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_handler_basic_functionality() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let mut handler = AuraHandlerFactory::for_testing(device_id);
        let mut ctx = AuraContext::for_testing(device_id);

        // Test supports_effect
        assert!(!handler.supports_effect(EffectType::Console)); // Our stub returns false

        // Test execution_mode
        assert_eq!(handler.execution_mode(), ExecutionMode::Testing);

        // Test session execution (should succeed for our stub)
        let session = LocalSessionType::new(1, "test".to_string());
        let result = handler.execute_session(session, &mut ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_typed_effect_execution() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let mut handler = AuraHandlerFactory::for_testing(device_id);
        let mut ctx = AuraContext::for_testing(device_id);

        // Test typed effect execution
        #[derive(serde::Serialize, serde::Deserialize)]
        struct TestParams {
            value: u32,
        }

        let params = TestParams { value: 42 };

        // This would normally execute an effect, but our stub handler will return an error
        let result: Result<String, _> = HandlerUtils::execute_typed_effect(
            handler.as_mut(),
            EffectType::Console,
            "print",
            params,
            &mut ctx,
        )
        .await;

        // Our stub returns UnsupportedEffect, which is expected
        assert!(result.is_err());
    }
}
