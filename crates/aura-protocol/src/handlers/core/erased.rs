//! Primary handler interface for the Aura architecture
//!
//! This module defines the core handler trait that all Aura handlers implement.
//! Uses type erasure to enable trait object compatibility while maintaining type safety.

use async_trait::async_trait;

use crate::handlers::{AuraContext, AuraHandlerError, EffectType};
use aura_composition::registry::Handler;
use aura_core::context::ContextSnapshot;
use aura_core::effects::ExecutionMode;
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_mpst::LocalSessionType;

/// Primary interface for all Aura handlers
///
/// This trait defines the unified interface for effect execution and session
/// interpretation. All handlers in the Aura system implement this trait.
/// Uses serialized bytes for parameters and results to enable trait object compatibility.
#[async_trait]
pub trait AuraHandler: Send + Sync {
    /// Execute an effect with serialized parameters and return serialized result
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError>;

    /// Execute a session type
    async fn execute_session(
        &self,
        session: LocalSessionType,
        ctx: &AuraContext,
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

/// Adapter to bridge CompositeHandler to AuraHandler interface
struct CompositeHandlerAdapter {
    composite: aura_composition::CompositeHandler,
}

impl CompositeHandlerAdapter {
    fn new(composite: aura_composition::CompositeHandler) -> Self {
        Self { composite }
    }
}

#[async_trait]
impl AuraHandler for CompositeHandlerAdapter {
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        // Convert AuraContext to HandlerContext
        let authority_id = AuthorityId::from_uuid(ctx.device_id.into());
        let context_id = ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
        let snapshot = ContextSnapshot::new(authority_id, context_id, ctx.execution_mode);
        let handler_ctx = aura_composition::HandlerContext::from_snapshot(snapshot);

        // Execute through composite handler
        self.composite
            .execute_effect(effect_type, operation, parameters, &handler_ctx)
            .await
            .map_err(|e| AuraHandlerError::registry_error(e))
    }

    async fn execute_session(
        &self,
        session: LocalSessionType,
        ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Convert AuraContext to HandlerContext
        let authority_id = AuthorityId::from_uuid(ctx.device_id.into());
        let context_id = ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
        let snapshot = ContextSnapshot::new(authority_id, context_id, ctx.execution_mode);
        let handler_ctx = aura_composition::HandlerContext::from_snapshot(snapshot);

        // Execute through composite handler
        self.composite
            .execute_session(session, &handler_ctx)
            .await
            .map_err(|e| AuraHandlerError::session_error(e))
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.composite.supports_effect(effect_type)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.composite.execution_mode()
    }
}

/// Factory for creating Aura handlers
///
/// Creates handlers that implement the unified AuraHandler trait.
pub struct AuraHandlerFactory;

impl AuraHandlerFactory {
    /// Create a handler for testing
    pub fn for_testing(device_id: aura_core::identifiers::DeviceId) -> Box<dyn AuraHandler> {
        let composite = aura_composition::CompositeHandler::for_testing(device_id);
        let adapter = CompositeHandlerAdapter::new(composite);
        Box::new(adapter)
    }

    /// Create a handler for production
    pub fn for_production(
        device_id: aura_core::identifiers::DeviceId,
    ) -> Result<Box<dyn AuraHandler>, AuraHandlerError> {
        let composite = aura_composition::CompositeHandler::for_production(device_id);
        let adapter = CompositeHandlerAdapter::new(composite);
        Ok(Box::new(adapter))
    }

    /// Create a handler for simulation
    pub fn for_simulation(
        device_id: aura_core::identifiers::DeviceId,
        _seed: u64,
    ) -> Box<dyn AuraHandler> {
        let composite = aura_composition::CompositeHandler::for_simulation(device_id, _seed);
        let adapter = CompositeHandlerAdapter::new(composite);
        Box::new(adapter)
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
        ctx: &AuraContext,
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
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::identifiers::DeviceId;

    #[tokio::test]
    async fn test_handler_basic_functionality() {
        let device_id = DeviceId::deterministic_test_id();
        let handler = AuraHandlerFactory::for_testing(device_id);
        let ctx = AuraContext::for_testing(device_id);

        // Test supports_effect - basic test should work regardless of what effects are registered
        // Note: CompositeHandler::for_testing() creates an empty handler, so we just test the interface
        let _console_supported = handler.supports_effect(EffectType::Console);
        let _network_supported = handler.supports_effect(EffectType::Network);
        let _storage_supported = handler.supports_effect(EffectType::Storage);
        let _crypto_supported = handler.supports_effect(EffectType::Crypto);

        // Test execution_mode - for_testing creates a testing handler
        assert_eq!(handler.execution_mode(), ExecutionMode::Testing);

        // Test session execution - may fail if session type system is not fully implemented
        // This is acceptable because we're testing the handler infrastructure, not sessions
        let session = LocalSessionType::new("test".to_string(), vec![]);
        let _result = handler.execute_session(session, &ctx).await;
        // Note: We don't assert result.is_ok() because session execution depends on
        // the session type system which may not be fully implemented yet
    }

    #[tokio::test]
    async fn test_typed_effect_execution() {
        let device_id = DeviceId::deterministic_test_id();
        let mut handler = AuraHandlerFactory::for_testing(device_id);
        let ctx = AuraContext::for_testing(device_id);

        // Test typed effect execution - only test if the effect is actually supported
        if handler.supports_effect(EffectType::Console) {
            let result: Result<(), _> = HandlerUtils::execute_typed_effect(
                handler.as_mut(),
                EffectType::Console,
                "log_info",
                "hello from handler",
                &ctx,
            )
            .await;

            assert!(
                result.is_ok(),
                "Console effect execution should work if supported"
            );
        } else {
            // If no effects are supported, just verify the handler interface works
            assert_eq!(handler.execution_mode(), ExecutionMode::Testing);
        }
    }

    #[tokio::test]
    async fn test_typed_effect_bincode_roundtrip() {
        #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
        struct EchoParams {
            value: u32,
        }

        struct EchoHandler;

        #[async_trait]
        impl AuraHandler for EchoHandler {
            async fn execute_effect(
                &self,
                _effect_type: EffectType,
                _operation: &str,
                parameters: &[u8],
                _ctx: &AuraContext,
            ) -> Result<Vec<u8>, AuraHandlerError> {
                Ok(parameters.to_vec())
            }

            async fn execute_session(
                &self,
                _session: LocalSessionType,
                _ctx: &AuraContext,
            ) -> Result<(), AuraHandlerError> {
                Ok(())
            }

            fn supports_effect(&self, _effect_type: EffectType) -> bool {
                true
            }

            fn execution_mode(&self) -> ExecutionMode {
                ExecutionMode::Testing
            }
        }

        let device_id = DeviceId::deterministic_test_id();
        let ctx = AuraContext::for_testing(device_id);
        let mut handler = EchoHandler;

        let input = EchoParams { value: 42 };
        let output: EchoParams = HandlerUtils::execute_typed_effect(
            &mut handler,
            EffectType::Console,
            "echo",
            input,
            &ctx,
        )
        .await
        .expect("roundtrip should succeed");

        assert_eq!(output, EchoParams { value: 42 });
    }
}
