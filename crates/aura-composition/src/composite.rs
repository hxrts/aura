//! Composite Handler for combining multiple effect handlers
//!
//! This module provides a composite handler that can delegate to multiple
//! specialized handlers based on effect type, enabling flexible composition
//! and modular handler architecture.

use async_trait::async_trait;
use std::collections::HashMap;

use crate::registry::{Handler, HandlerContext, HandlerError, RegistrableHandler};
use aura_core::{DeviceId, EffectType, ExecutionMode, LocalSessionType};

/// A composite handler that delegates to specialized handlers based on effect type
pub struct CompositeHandler {
    /// Map of effect types to their handlers
    handlers: HashMap<EffectType, Box<dyn Handler>>,
    /// Default execution mode
    execution_mode: ExecutionMode,
    /// Device ID
    device_id: DeviceId,
}

impl CompositeHandler {
    // Adapter-style composite
    /// Create a new composite handler
    pub fn new(device_id: DeviceId, execution_mode: ExecutionMode) -> Self {
        Self {
            handlers: HashMap::new(),
            execution_mode,
            device_id,
        }
    }

    /// Create a composite handler for testing
    pub fn for_testing(device_id: DeviceId) -> Self {
        Self::new(device_id, ExecutionMode::Testing)
    }

    /// Create a composite handler for production
    pub fn for_production(device_id: DeviceId) -> Self {
        Self::new(device_id, ExecutionMode::Production)
    }

    /// Create a composite handler for simulation
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self::new(device_id, ExecutionMode::Simulation { seed })
    }

    /// Register a handler for a specific effect type
    pub fn register_handler(
        &mut self,
        effect_type: EffectType,
        handler: Box<dyn Handler>,
    ) -> Result<(), CompositeError> {
        if !handler.supports_effect(effect_type) {
            return Err(CompositeError::UnsupportedEffect { effect_type });
        }

        self.handlers.insert(effect_type, handler);
        Ok(())
    }

    /// Unregister a handler for a specific effect type
    pub fn unregister_handler(&mut self, effect_type: EffectType) -> Option<Box<dyn Handler>> {
        self.handlers.remove(&effect_type)
    }

    /// Check if a handler is registered for an effect type
    pub fn has_handler(&self, effect_type: EffectType) -> bool {
        self.handlers.contains_key(&effect_type)
    }

    /// Get all registered effect types
    pub fn registered_effect_types(&self) -> Vec<EffectType> {
        self.handlers.keys().copied().collect()
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

/// Error type for composite handler operations
#[derive(Debug, thiserror::Error)]
pub enum CompositeError {
    /// Effect type not supported by handler
    #[error("Effect type {effect_type:?} not supported by handler")]
    UnsupportedEffect { effect_type: EffectType },

    /// No handler registered for effect type
    #[error("No handler registered for effect type {effect_type:?}")]
    NoHandlerRegistered { effect_type: EffectType },

    /// Handler execution failed
    #[error("Handler execution failed for effect type {effect_type:?}")]
    HandlerExecutionFailed {
        effect_type: EffectType,
        #[source]
        source: HandlerError,
    },
}

#[async_trait]
impl Handler for CompositeHandler {
    // Adapter-style composite
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        match self.handlers.get(&effect_type) {
            Some(handler) => {
                handler
                    .execute_effect(effect_type, operation, parameters, ctx)
                    .await
            }
            None => Err(HandlerError::UnsupportedEffect { effect_type }),
        }
    }

    async fn execute_session(
        &self,
        session: LocalSessionType,
        ctx: &HandlerContext,
    ) -> Result<(), HandlerError> {
        // For now, delegate to choreographic effect handler if available
        if let Some(handler) = self.handlers.get(&EffectType::Choreographic) {
            handler.execute_session(session, ctx).await
        } else {
            // Return a session execution error if no choreographic handler is available
            Err(HandlerError::SessionExecution {
                source: "No choreographic handler registered".into(),
            })
        }
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.handlers
            .get(&effect_type)
            .map(|h| h.supports_effect(effect_type))
            .unwrap_or(false)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

/// Builder for creating composite handlers
pub struct CompositeHandlerBuilder {
    device_id: DeviceId,
    execution_mode: ExecutionMode,
    handlers: HashMap<EffectType, Box<dyn Handler>>,
}

impl CompositeHandlerBuilder {
    /// Create a new builder
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Testing,
            handlers: HashMap::new(),
        }
    }

    /// Set execution mode
    pub fn execution_mode(mut self, mode: ExecutionMode) -> Self {
        self.execution_mode = mode;
        self
    }

    /// Add a handler for an effect type
    pub fn with_handler(
        mut self,
        effect_type: EffectType,
        handler: Box<dyn Handler>,
    ) -> Result<Self, CompositeError> {
        if !handler.supports_effect(effect_type) {
            return Err(CompositeError::UnsupportedEffect { effect_type });
        }
        self.handlers.insert(effect_type, handler);
        Ok(self)
    }

    /// Build the composite handler
    pub fn build(self) -> CompositeHandler {
        let mut composite = CompositeHandler::new(self.device_id, self.execution_mode);
        for (effect_type, handler) in self.handlers {
            // We know the handler supports the effect type from the with_handler check
            composite.handlers.insert(effect_type, handler);
        }
        composite
    }
}

/// Adapter to make CompositeHandler work as RegistrableHandler
pub struct CompositeHandlerAdapter {
    composite: CompositeHandler,
}

impl CompositeHandlerAdapter {
    /// Create a new adapter
    pub fn new(composite: CompositeHandler) -> Self {
        Self { composite }
    }

    /// Create adapter for testing
    pub fn for_testing(device_id: DeviceId) -> Self {
        Self::new(CompositeHandler::for_testing(device_id))
    }

    /// Create adapter for production
    pub fn for_production(device_id: DeviceId) -> Self {
        Self::new(CompositeHandler::for_production(device_id))
    }

    /// Create adapter for simulation
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self::new(CompositeHandler::for_simulation(device_id, seed))
    }

    /// Register a handler
    pub fn register_handler(
        &mut self,
        effect_type: EffectType,
        handler: Box<dyn Handler>,
    ) -> Result<(), CompositeError> {
        self.composite.register_handler(effect_type, handler)
    }

    /// Get the underlying composite handler
    pub fn into_composite(self) -> CompositeHandler {
        self.composite
    }

    /// Get a reference to the composite handler
    pub fn composite(&self) -> &CompositeHandler {
        &self.composite
    }

    /// Get a mutable reference to the composite handler
    pub fn composite_mut(&mut self) -> &mut CompositeHandler {
        &mut self.composite
    }
}

#[async_trait]
impl Handler for CompositeHandlerAdapter {
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        self.composite
            .execute_effect(effect_type, operation, parameters, ctx)
            .await
    }

    async fn execute_session(
        &self,
        session: LocalSessionType,
        ctx: &HandlerContext,
    ) -> Result<(), HandlerError> {
        self.composite.execute_session(session, ctx).await
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.composite.supports_effect(effect_type)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.composite.execution_mode()
    }
}

#[async_trait]
impl RegistrableHandler for CompositeHandlerAdapter {
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        self.execute_effect(effect_type, operation, parameters, ctx)
            .await
    }

    fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
        // Return the standard operation mapping regardless of whether handlers are registered
        // This allows for capability discovery even before handlers are initialized
        match effect_type {
            EffectType::Console => vec![
                String::from("log_info"),
                String::from("log_warn"),
                String::from("log_error"),
                String::from("log_debug"),
            ],
            EffectType::Random => vec![
                String::from("random_bytes"),
                String::from("random_bytes_32"),
                String::from("random_u64"),
            ],
            EffectType::Crypto => vec![
                String::from("hkdf_derive"),
                String::from("ed25519_generate_keypair"),
                String::from("ed25519_sign"),
                String::from("ed25519_verify"),
            ],
            EffectType::Network => vec![
                String::from("send_to_peer"),
                String::from("broadcast"),
                String::from("receive"),
            ],
            EffectType::Storage => vec![
                String::from("store"),
                String::from("retrieve"),
                String::from("remove"),
                String::from("list_keys"),
            ],
            EffectType::Time => vec![
                String::from("current_epoch"),
                String::from("current_timestamp"),
                String::from("sleep_ms"),
            ],
            _ => Vec::new(),
        }
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.composite.supports_effect(effect_type)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.composite.execution_mode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Handler;

    #[test]
    fn test_composite_handler_creation() {
        let device_id = DeviceId::new();

        let handler = CompositeHandler::for_testing(device_id);
        assert_eq!(handler.execution_mode(), ExecutionMode::Testing);
        assert_eq!(handler.device_id(), device_id);

        let handler = CompositeHandler::for_production(device_id);
        assert_eq!(handler.execution_mode(), ExecutionMode::Production);

        let handler = CompositeHandler::for_simulation(device_id, 42);
        assert_eq!(
            handler.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
    }

    #[test]
    fn test_composite_handler_builder() {
        let device_id = DeviceId::new();

        let builder =
            CompositeHandlerBuilder::new(device_id).execution_mode(ExecutionMode::Production);

        // Note: We can't easily test handler registration here without mock handlers
        // In a real test, we would create mock handlers and register them

        let composite = builder.build();
        assert_eq!(composite.execution_mode(), ExecutionMode::Production);
        assert_eq!(composite.device_id(), device_id);
    }

    #[test]
    fn test_composite_handler_adapter() {
        let device_id = DeviceId::new();

        let adapter = CompositeHandlerAdapter::for_testing(device_id);
        assert_eq!(Handler::execution_mode(&adapter), ExecutionMode::Testing);

        let adapter = CompositeHandlerAdapter::for_production(device_id);
        assert_eq!(Handler::execution_mode(&adapter), ExecutionMode::Production);

        let adapter = CompositeHandlerAdapter::for_simulation(device_id, 42);
        assert_eq!(
            Handler::execution_mode(&adapter),
            ExecutionMode::Simulation { seed: 42 }
        );
    }

    #[test]
    fn test_handler_registration() {
        let device_id = DeviceId::new();
        let composite = CompositeHandler::for_testing(device_id);

        // Initially no handlers registered
        assert!(!composite.has_handler(EffectType::Console));
        assert!(composite.registered_effect_types().is_empty());

        // Note: Would register actual handlers in a complete test
        // For now, we just test the registration infrastructure exists
    }

    #[test]
    fn test_supported_operations() {
        let device_id = DeviceId::new();
        let adapter = CompositeHandlerAdapter::for_testing(device_id);

        // Test that the operation mapping exists (even without registered handlers)
        let console_ops = adapter.supported_operations(EffectType::Console);
        assert!(console_ops.contains(&"log_info".to_string()));

        let random_ops = adapter.supported_operations(EffectType::Random);
        assert!(random_ops.contains(&"random_bytes".to_string()));

        // Unsupported effect type should return empty list
        let unknown_ops = adapter.supported_operations(EffectType::PropertyChecking);
        assert!(unknown_ops.is_empty());
    }
}
