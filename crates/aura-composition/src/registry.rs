//! Effect registry system for dynamic effect dispatch
//!
//! This module provides a registry-based system for dispatching effects
//! to appropriate handlers. The registry enables dynamic composition
//! and runtime reconfiguration of effect handlers.

use async_trait::async_trait;
use aura_core::{AuthorityId, ContextId, ContextSnapshot, EffectType, ExecutionMode, SessionId};
use aura_mpst::LocalSessionType;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

/// Simplified context for handler execution
#[derive(Debug, Clone)]
pub struct HandlerContext {
    pub authority_id: AuthorityId,
    pub context_id: ContextId,
    pub execution_mode: ExecutionMode,
    pub session_id: SessionId,
    pub operation_id: Uuid,
    pub metadata: HashMap<String, String>,
}

impl HandlerContext {
    // Registry helper
    /// Create a new handler context
    pub fn new(
        authority_id: AuthorityId,
        context_id: ContextId,
        execution_mode: ExecutionMode,
    ) -> Self {
        let operation_id = Uuid::new_v4();
        Self {
            authority_id,
            context_id,
            execution_mode,
            session_id: SessionId::new(),
            operation_id,
            metadata: HashMap::new(),
        }
    }

    /// Create a new handler context from a lightweight snapshot.
    pub fn from_snapshot(snapshot: ContextSnapshot) -> Self {
        Self {
            authority_id: snapshot.authority_id(),
            context_id: snapshot.context_id(),
            execution_mode: snapshot.execution_mode(),
            session_id: snapshot.session_id(),
            operation_id: Uuid::new_v4(),
            metadata: HashMap::new(),
        }
    }

    /// Set session ID
    pub fn with_session_id(mut self, session_id: SessionId) -> Self {
        self.session_id = session_id;
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Error type for handler operations
#[derive(Debug, Error)]
pub enum HandlerError {
    /// Effect type not supported
    #[error("Effect {effect_type:?} not supported")]
    UnsupportedEffect { effect_type: EffectType },

    /// Operation not found within effect type
    #[error("Operation '{operation}' not found in effect {effect_type:?}")]
    UnknownOperation {
        effect_type: EffectType,
        operation: String,
    },

    /// Effect parameter serialization failed
    #[error("Failed to serialize parameters for {effect_type:?}.{operation}")]
    EffectSerialization {
        effect_type: EffectType,
        operation: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Effect parameter deserialization failed
    #[error("Failed to deserialize parameters for {effect_type:?}.{operation}")]
    EffectDeserialization {
        effect_type: EffectType,
        operation: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Session execution failed
    #[error("Session type execution failed")]
    SessionExecution {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Context operation failed
    #[error("Context operation failed: {message}")]
    ContextError { message: String },

    /// Registry operation failed
    #[error("Registry operation failed")]
    RegistryError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Execution failed
    #[error("Effect execution failed")]
    ExecutionFailed {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Primary interface for all Aura handlers
///
/// This trait defines the unified interface for effect execution and session
/// interpretation. All handlers in the Aura system implement this trait.
/// Uses serialized bytes for parameters and results to enable trait object compatibility.
#[async_trait]
pub trait Handler: Send + Sync {
    /// Execute an effect with serialized parameters and return serialized result
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError>;

    /// Execute a session type
    async fn execute_session(
        &self,
        session: LocalSessionType,
        ctx: &HandlerContext,
    ) -> Result<(), HandlerError>;

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

/// Error type for registry operations
#[derive(Debug, Error)]
pub enum RegistryError {
    /// Effect type not registered
    #[error("Effect type {effect_type:?} not registered")]
    EffectTypeNotRegistered { effect_type: EffectType },

    /// Operation not supported by registered handler
    #[error("Operation '{operation}' not supported by handler for {effect_type:?}")]
    OperationNotSupported {
        effect_type: EffectType,
        operation: String,
    },

    /// Handler registration failed
    #[error("Failed to register handler for {effect_type:?}")]
    RegistrationFailed {
        effect_type: EffectType,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Handler execution failed
    #[error("Handler execution failed for {effect_type:?}")]
    HandlerExecutionFailed {
        effect_type: EffectType,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Parameter deserialization failed
    #[error("Failed to deserialize result from {effect_type:?} operation '{operation}'")]
    ParameterDeserialization {
        effect_type: EffectType,
        operation: String,
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
        ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError>;

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

    /// Number of registered handlers (test helper)
    pub fn handlers_len(&self) -> usize {
        self.handlers.len()
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
        _ctx: &mut HandlerContext,
    ) -> Result<(), RegistryError> {
        Err(RegistryError::OperationNotSupported {
            effect_type: EffectType::Choreographic,
            operation: "execute_session".to_string(),
        })
    }

    /// Get the execution mode of the registry
    pub fn execution_mode(&self) -> ExecutionMode {
        self.default_execution_mode
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
impl Handler for EffectRegistry {
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &HandlerContext,
    ) -> Result<Vec<u8>, HandlerError> {
        // Route to the appropriate registered handler
        if let Some(handler) = self.handlers.get(&effect_type) {
            handler
                .execute_operation_bytes(effect_type, operation, parameters, ctx)
                .await
        } else {
            Err(HandlerError::UnsupportedEffect { effect_type })
        }
    }

    async fn execute_session(
        &self,
        _session: LocalSessionType,
        _ctx: &HandlerContext,
    ) -> Result<(), HandlerError> {
        let err = std::io::Error::other("session execution not wired in registry");
        Err(HandlerError::SessionExecution {
            source: Box::new(err),
        })
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
        // Adapter test shim
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
    impl Handler for MockRegistrableHandler {
        // Adapter test shim
        async fn execute_effect(
            &self,
            effect_type: EffectType,
            operation: &str,
            _parameters: &[u8],
            _ctx: &HandlerContext,
        ) -> Result<Vec<u8>, HandlerError> {
            if self.effect_type == effect_type && self.operations.contains(&operation.to_string()) {
                // Mock successful result
                bincode::serialize(&serde_json::Value::String("mock_result".to_string())).map_err(
                    |e| HandlerError::EffectSerialization {
                        effect_type,
                        operation: operation.to_string(),
                        source: e.into(),
                    },
                )
            } else {
                Err(HandlerError::UnsupportedEffect { effect_type })
            }
        }

        async fn execute_session(
            &self,
            _session: LocalSessionType,
            _ctx: &HandlerContext,
        ) -> Result<(), HandlerError> {
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
        // Adapter test shim
        async fn execute_operation_bytes(
            &self,
            _effect_type: EffectType,
            operation: &str,
            _parameters: &[u8],
            _ctx: &HandlerContext,
        ) -> Result<Vec<u8>, HandlerError> {
            if self.operations.contains(&operation.to_string()) {
                // Mock successful operation - return serialized mock result
                let mock_result = serde_json::Value::String("mock_result".to_string());
                bincode::serialize(&mock_result).map_err(|e| HandlerError::EffectSerialization {
                    effect_type: self.effect_type,
                    operation: operation.to_string(),
                    source: e.into(),
                })
            } else {
                Err(HandlerError::UnknownOperation {
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
    fn test_handler_context_operation_id_unique() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let context_id = ContextId::new_from_entropy([2u8; 32]);
        let ctx1 = HandlerContext::new(authority_id, context_id, ExecutionMode::Testing);
        let ctx2 = HandlerContext::new(authority_id, context_id, ExecutionMode::Testing);

        assert_ne!(ctx1.operation_id, ctx2.operation_id);
    }
}
