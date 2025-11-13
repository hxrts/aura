//! Effect registry system for dynamic effect dispatch
//!
//! This module provides a registry-based system for dispatching effects
//! to appropriate handlers. The registry enables dynamic composition
//! and runtime reconfiguration of effect handlers.

use async_trait::async_trait;
use std::collections::HashMap;
use thiserror::Error;

use super::context_immutable::AuraContext;
use super::{AuraHandler, AuraHandlerError, EffectType, ExecutionMode};
use aura_core::LocalSessionType;

/// Error type for registry operations
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Effect type not registered
    #[error("Effect type {effect_type:?} not registered")]
    EffectTypeNotRegistered {
        /// The effect type that is not registered
        effect_type: EffectType,
    },

    /// Operation not supported by registered handler
    #[error("Operation '{operation}' not supported by handler for {effect_type:?}")]
    OperationNotSupported {
        /// The effect type being queried
        effect_type: EffectType,
        /// The operation name that is not supported
        operation: String,
    },

    /// Handler registration failed
    #[error("Failed to register handler for {effect_type:?}")]
    RegistrationFailed {
        /// The effect type for which registration failed
        effect_type: EffectType,
        /// Underlying error from registration
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Handler execution failed
    #[error("Handler execution failed for {effect_type:?}")]
    HandlerExecutionFailed {
        /// The effect type for which execution failed
        effect_type: EffectType,
        /// Underlying error from handler execution
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Parameter deserialization failed
    #[error("Failed to deserialize result from {effect_type:?} operation '{operation}'")]
    ParameterDeserialization {
        /// The effect type being processed
        effect_type: EffectType,
        /// The operation name being executed
        operation: String,
        /// Underlying error from deserialization
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl RegistryError {
    /// Create a registration failed error
    pub fn registration_failed(
        effect_type: EffectType,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::RegistrationFailed {
            effect_type,
            source: Box::new(source),
        }
    }

    /// Create a handler execution failed error
    pub fn handler_execution_failed(
        effect_type: EffectType,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::HandlerExecutionFailed {
            effect_type,
            source: Box::new(source),
        }
    }
}

/// Trait for handlers that can be registered in the effect registry
///
/// This trait provides a type-erased interface for effect execution
/// that can be used in trait objects.
#[async_trait]
pub trait RegistrableHandler: Send + Sync {
    /// Execute a specific operation within an effect type
    ///
    /// This method is called by the registry to execute specific operations
    /// after the effect has been dispatched to the correct handler.
    /// Returns serialized result bytes.
    async fn execute_operation_bytes(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError>;

    /// Get the list of operations supported by this handler for a given effect type
    ///
    /// Returns the operation names that this handler can execute for the
    /// specified effect type. Used for capability discovery and validation.
    fn supported_operations(&self, effect_type: EffectType) -> Vec<String>;

    /// Check if a specific operation is supported
    fn supports_operation(&self, effect_type: EffectType, operation: &str) -> bool {
        self.supported_operations(effect_type)
            .contains(&operation.to_string())
    }

    /// Check if this handler supports the given effect type
    fn supports_effect(&self, effect_type: EffectType) -> bool;

    /// Get the execution mode of this handler
    fn execution_mode(&self) -> ExecutionMode;
}

/// Registry for effect handlers
///
/// The registry maintains a mapping from effect types to handlers and
/// provides dynamic dispatch capabilities for effect execution.
pub struct EffectRegistry {
    /// Registered handlers by effect type
    handlers: HashMap<EffectType, Box<dyn RegistrableHandler>>,
    /// Default execution mode for the registry
    default_execution_mode: ExecutionMode,
}

impl std::fmt::Debug for EffectRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EffectRegistry")
            .field(
                "handlers",
                &format!("HashMap with {} entries", self.handlers.len()),
            )
            .field("default_execution_mode", &self.default_execution_mode)
            .finish()
    }
}

impl EffectRegistry {
    /// Create a new effect registry
    pub fn new(default_execution_mode: ExecutionMode) -> Self {
        Self {
            handlers: HashMap::new(),
            default_execution_mode,
        }
    }

    /// Register a handler for a specific effect type
    ///
    /// # Errors
    ///
    /// Returns an error if the handler doesn't support the effect type
    /// or if there's a conflict with an existing registration.
    pub fn register_handler(
        &mut self,
        effect_type: EffectType,
        handler: Box<dyn RegistrableHandler>,
    ) -> Result<(), RegistryError> {
        // Validate that the handler supports this effect type
        if !handler.supports_effect(effect_type) {
            return Err(RegistryError::registration_failed(
                effect_type,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Handler does not support the specified effect type",
                ),
            ));
        }

        // Register the handler
        self.handlers.insert(effect_type, handler);
        Ok(())
    }

    /// Unregister a handler for a specific effect type
    pub fn unregister_handler(
        &mut self,
        effect_type: EffectType,
    ) -> Option<Box<dyn RegistrableHandler>> {
        self.handlers.remove(&effect_type)
    }

    /// Check if a handler is registered for an effect type
    pub fn is_registered(&self, effect_type: EffectType) -> bool {
        self.handlers.contains_key(&effect_type)
    }

    /// Get all registered effect types
    pub fn registered_effect_types(&self) -> Vec<EffectType> {
        self.handlers.keys().copied().collect()
    }

    /// Get supported operations for an effect type
    pub fn supported_operations(
        &self,
        effect_type: EffectType,
    ) -> Result<Vec<String>, RegistryError> {
        match self.handlers.get(&effect_type) {
            Some(handler) => Ok(handler.supported_operations(effect_type)),
            None => Err(RegistryError::EffectTypeNotRegistered { effect_type }),
        }
    }

    /// Check if an operation is supported for an effect type
    pub fn supports_operation(&self, effect_type: EffectType, operation: &str) -> bool {
        self.handlers
            .get(&effect_type)
            .map(|h| h.supports_operation(effect_type, operation))
            .unwrap_or(false)
    }

    /// Execute a session type through the registry
    ///
    /// Routes session execution to an appropriate handler. For session types,
    /// this typically uses the choreographic effect handler.
    pub async fn execute_session(
        &mut self,
        _session: LocalSessionType,
        _ctx: &mut AuraContext,
    ) -> Result<(), RegistryError> {
        // TODO fix - Simplified stub - session execution would normally use choreographic handlers
        Ok(())
    }

    /// Get a summary of registry capabilities
    pub fn capability_summary(&self) -> RegistryCapabilities {
        let mut capabilities = RegistryCapabilities {
            registered_effects: Vec::new(),
            total_operations: 0,
            execution_modes: Vec::new(),
        };

        for (effect_type, handler) in &self.handlers {
            let operations = handler.supported_operations(*effect_type);
            let operation_count = operations.len();
            capabilities.registered_effects.push(EffectCapability {
                effect_type: *effect_type,
                operation_count,
                operations,
            });
            capabilities.total_operations += operation_count;

            let mode = handler.execution_mode();
            if !capabilities.execution_modes.contains(&mode) {
                capabilities.execution_modes.push(mode);
            }
        }

        capabilities
    }
}

#[async_trait]
impl AuraHandler for EffectRegistry {
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        // Route to the appropriate registered handler
        if let Some(handler) = self.handlers.get(&effect_type) {
            handler
                .execute_operation_bytes(effect_type, operation, parameters, ctx)
                .await
        } else {
            Err(AuraHandlerError::UnsupportedEffect { effect_type })
        }
    }

    async fn execute_session(
        &self,
        _session: LocalSessionType,
        _ctx: &AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // TODO fix - Simplified stub - session execution would normally route to choreographic handlers
        Ok(())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.is_registered(effect_type)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.default_execution_mode
    }
}

/// Summary of registry capabilities
#[derive(Debug, Clone)]
pub struct RegistryCapabilities {
    /// All registered effect types with their capabilities
    pub registered_effects: Vec<EffectCapability>,
    /// Total number of operations across all effects
    pub total_operations: usize,
    /// Execution modes of registered handlers
    pub execution_modes: Vec<ExecutionMode>,
}

/// Capability information for a single effect type
#[derive(Debug, Clone)]
pub struct EffectCapability {
    /// The effect type
    pub effect_type: EffectType,
    /// Number of supported operations
    pub operation_count: usize,
    /// List of supported operation names
    pub operations: Vec<String>,
}

impl RegistryCapabilities {
    /// Check if a specific effect type is registered
    pub fn has_effect_type(&self, effect_type: EffectType) -> bool {
        self.registered_effects
            .iter()
            .any(|cap| cap.effect_type == effect_type)
    }

    /// Get capability information for a specific effect type
    pub fn get_effect_capability(&self, effect_type: EffectType) -> Option<&EffectCapability> {
        self.registered_effects
            .iter()
            .find(|cap| cap.effect_type == effect_type)
    }

    /// Check if a specific operation is supported
    pub fn supports_operation(&self, effect_type: EffectType, operation: &str) -> bool {
        self.get_effect_capability(effect_type)
            .map(|cap| cap.operations.contains(&operation.to_string()))
            .unwrap_or(false)
    }

    /// Get the number of registered effect types
    pub fn effect_type_count(&self) -> usize {
        self.registered_effects.len()
    }

    /// Check if the registry supports a specific execution mode
    pub fn supports_execution_mode(&self, mode: ExecutionMode) -> bool {
        self.execution_modes.contains(&mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock registrable handler for testing
    struct MockRegistrableHandler {
        effect_type: EffectType,
        operations: Vec<String>,
        execution_mode: ExecutionMode,
    }

    impl MockRegistrableHandler {
        fn new(
            effect_type: EffectType,
            operations: Vec<&str>,
            execution_mode: ExecutionMode,
        ) -> Self {
            Self {
                effect_type,
                operations: operations.into_iter().map(|s| s.to_string()).collect(),
                execution_mode,
            }
        }
    }

    #[async_trait]
    impl AuraHandler for MockRegistrableHandler {
        async fn execute_effect(
            &mut self,
            effect_type: EffectType,
            operation: &str,
            _parameters: &[u8],
            _ctx: &mut AuraContext,
        ) -> Result<Vec<u8>, AuraHandlerError> {
            if self.effect_type == effect_type && self.operations.contains(&operation.to_string()) {
                // Mock successful result
                bincode::serialize(&serde_json::Value::String("mock_result".to_string())).map_err(
                    |e| AuraHandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: e.into(),
                    },
                )
            } else {
                Err(AuraHandlerError::UnsupportedEffect { effect_type })
            }
        }

        async fn execute_session(
            &mut self,
            _session: LocalSessionType,
            _ctx: &mut AuraContext,
        ) -> Result<(), AuraHandlerError> {
            Ok(())
        }

        fn supports_effect(&self, effect_type: EffectType) -> bool {
            self.effect_type == effect_type
        }

        fn execution_mode(&self) -> ExecutionMode {
            self.execution_mode
        }
    }

    #[async_trait]
    impl RegistrableHandler for MockRegistrableHandler {
        async fn execute_operation_bytes(
            &mut self,
            _effect_type: EffectType,
            operation: &str,
            _parameters: &[u8],
            _ctx: &mut AuraContext,
        ) -> Result<Vec<u8>, AuraHandlerError> {
            if self.operations.contains(&operation.to_string()) {
                // Mock successful operation - return serialized mock result
                let mock_result = serde_json::Value::String("mock_result".to_string());
                bincode::serialize(&mock_result).map_err(|e| {
                    AuraHandlerError::EffectSerialization {
                        effect_type: self.effect_type,
                        operation: operation.to_string(),
                        source: e.into(),
                    }
                })
            } else {
                Err(AuraHandlerError::UnknownOperation {
                    effect_type: self.effect_type,
                    operation: operation.to_string(),
                })
            }
        }

        fn supported_operations(&self, effect_type: EffectType) -> Vec<String> {
            if self.effect_type == effect_type {
                self.operations.clone()
            } else {
                Vec::new()
            }
        }

        fn supports_effect(&self, effect_type: EffectType) -> bool {
            self.effect_type == effect_type
        }

        fn execution_mode(&self) -> ExecutionMode {
            self.execution_mode
        }
    }

    #[test]
    fn test_registry_creation() {
        let registry = EffectRegistry::new(ExecutionMode::Testing);
        assert_eq!(registry.execution_mode(), ExecutionMode::Testing);
        assert!(registry.registered_effect_types().is_empty());
    }

    #[test]
    fn test_handler_registration() {
        let mut registry = EffectRegistry::new(ExecutionMode::Testing);

        let handler = Box::new(MockRegistrableHandler::new(
            EffectType::Crypto,
            vec!["hash", "sign", "verify"],
            ExecutionMode::Testing,
        ));

        // Register handler
        registry
            .register_handler(EffectType::Crypto, handler)
            .unwrap();

        assert!(registry.is_registered(EffectType::Crypto));
        assert!(!registry.is_registered(EffectType::Network));

        let effect_types = registry.registered_effect_types();
        assert_eq!(effect_types.len(), 1);
        assert!(effect_types.contains(&EffectType::Crypto));
    }

    #[test]
    fn test_operation_support() {
        let mut registry = EffectRegistry::new(ExecutionMode::Testing);

        let handler = Box::new(MockRegistrableHandler::new(
            EffectType::Crypto,
            vec!["hash", "sign", "verify"],
            ExecutionMode::Testing,
        ));

        registry
            .register_handler(EffectType::Crypto, handler)
            .unwrap();

        // Test supported operations
        let operations = registry.supported_operations(EffectType::Crypto).unwrap();
        assert_eq!(operations.len(), 3);
        assert!(operations.contains(&"hash".to_string()));
        assert!(operations.contains(&"sign".to_string()));
        assert!(operations.contains(&"verify".to_string()));

        // Test operation support checks
        assert!(registry.supports_operation(EffectType::Crypto, "hash"));
        assert!(registry.supports_operation(EffectType::Crypto, "sign"));
        assert!(!registry.supports_operation(EffectType::Crypto, "encrypt"));
        assert!(!registry.supports_operation(EffectType::Network, "send"));
    }

    #[test]
    fn test_handler_registration_validation() {
        let mut registry = EffectRegistry::new(ExecutionMode::Testing);

        // Try to register handler for wrong effect type
        let handler = Box::new(MockRegistrableHandler::new(
            EffectType::Crypto,
            vec!["hash"],
            ExecutionMode::Testing,
        ));

        // This should fail because the handler only supports Crypto but we're registering for Network
        let result = registry.register_handler(EffectType::Network, handler);
        assert!(result.is_err());

        match result.unwrap_err() {
            RegistryError::RegistrationFailed { effect_type, .. } => {
                assert_eq!(effect_type, EffectType::Network);
            }
            _ => panic!("Expected RegistrationFailed error"),
        }
    }

    #[test]
    fn test_capability_summary() {
        let mut registry = EffectRegistry::new(ExecutionMode::Testing);

        // Register multiple handlers
        let crypto_handler = Box::new(MockRegistrableHandler::new(
            EffectType::Crypto,
            vec!["hash", "sign"],
            ExecutionMode::Testing,
        ));
        let network_handler = Box::new(MockRegistrableHandler::new(
            EffectType::Network,
            vec!["send", "receive", "broadcast"],
            ExecutionMode::Production,
        ));

        registry
            .register_handler(EffectType::Crypto, crypto_handler)
            .unwrap();
        registry
            .register_handler(EffectType::Network, network_handler)
            .unwrap();

        let capabilities = registry.capability_summary();

        assert_eq!(capabilities.effect_type_count(), 2);
        assert_eq!(capabilities.total_operations, 5); // 2 + 3
        assert!(capabilities.has_effect_type(EffectType::Crypto));
        assert!(capabilities.has_effect_type(EffectType::Network));
        assert!(!capabilities.has_effect_type(EffectType::Storage));

        // Test operation support
        assert!(capabilities.supports_operation(EffectType::Crypto, "hash"));
        assert!(capabilities.supports_operation(EffectType::Network, "broadcast"));
        assert!(!capabilities.supports_operation(EffectType::Crypto, "encrypt"));

        // Test execution modes
        assert!(capabilities.supports_execution_mode(ExecutionMode::Testing));
        assert!(capabilities.supports_execution_mode(ExecutionMode::Production));
        assert!(!capabilities.supports_execution_mode(ExecutionMode::Simulation { seed: 42 }));
    }

    #[test]
    fn test_handler_unregistration() {
        let mut registry = EffectRegistry::new(ExecutionMode::Testing);

        let handler = Box::new(MockRegistrableHandler::new(
            EffectType::Crypto,
            vec!["hash"],
            ExecutionMode::Testing,
        ));

        registry
            .register_handler(EffectType::Crypto, handler)
            .unwrap();
        assert!(registry.is_registered(EffectType::Crypto));

        let removed_handler = registry.unregister_handler(EffectType::Crypto);
        assert!(removed_handler.is_some());
        assert!(!registry.is_registered(EffectType::Crypto));

        // Unregistering again should return None
        let removed_again = registry.unregister_handler(EffectType::Crypto);
        assert!(removed_again.is_none());
    }
}
