//! Effect registry for dynamic handler lookup
//!
//! Provides dynamic registry for effect handlers, enabling runtime composition
//! and protocol-specific handler selection in the authority-centric architecture.

use aura_core::effects::ExecutionMode;
use aura_core::identifiers::AuthorityId;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Dynamic registry for effect handlers
#[derive(Debug)]
pub struct EffectRegistry {
    handlers: Arc<RwLock<HashMap<EffectKey, Arc<dyn std::any::Any + Send + Sync>>>>,
    execution_mode: ExecutionMode,
}

impl EffectRegistry {
    /// Create a new effect registry
    pub fn new(execution_mode: ExecutionMode) -> Self {
        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
            execution_mode,
        }
    }

    /// Register an effect handler
    pub fn register<T: Send + Sync + 'static>(
        &self,
        effect_type: String,
        operation: String,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        let key = EffectKey::new(effect_type, operation);
        
        let mut handlers = self.handlers.write()
            .map_err(|_| EffectRegistryError::LockError)?;
        
        handlers.insert(key, Arc::new(handler));
        Ok(())
    }

    /// Get an effect handler
    pub fn get<T: Send + Sync + 'static>(
        &self,
        effect_type: &str,
        operation: &str,
    ) -> Result<Option<Arc<T>>, EffectRegistryError> {
        let key = EffectKey::new(effect_type.to_string(), operation.to_string());
        
        let handlers = self.handlers.read()
            .map_err(|_| EffectRegistryError::LockError)?;
        
        Ok(handlers.get(&key)
            .and_then(|handler| handler.clone().downcast::<T>().ok()))
    }

    /// Check if an effect handler is registered
    pub fn has_handler(&self, effect_type: &str, operation: &str) -> bool {
        let key = EffectKey::new(effect_type.to_string(), operation.to_string());
        
        self.handlers.read()
            .map(|handlers| handlers.contains_key(&key))
            .unwrap_or(false)
    }

    /// Get all registered effect types
    pub fn effect_types(&self) -> Vec<String> {
        self.handlers.read()
            .map(|handlers| {
                handlers.keys()
                    .map(|key| key.effect_type.clone())
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get operations for an effect type
    pub fn operations(&self, effect_type: &str) -> Vec<String> {
        self.handlers.read()
            .map(|handlers| {
                handlers.keys()
                    .filter(|key| key.effect_type == effect_type)
                    .map(|key| key.operation.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clear all handlers
    pub fn clear(&self) -> Result<(), EffectRegistryError> {
        let mut handlers = self.handlers.write()
            .map_err(|_| EffectRegistryError::LockError)?;
        
        handlers.clear();
        Ok(())
    }

    /// Get the execution mode
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode.clone()
    }
}

impl Clone for EffectRegistry {
    fn clone(&self) -> Self {
        Self {
            handlers: self.handlers.clone(),
            execution_mode: self.execution_mode.clone(),
        }
    }
}

/// Key for indexing effect handlers
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct EffectKey {
    effect_type: String,
    operation: String,
}

impl EffectKey {
    fn new(effect_type: String, operation: String) -> Self {
        Self {
            effect_type,
            operation,
        }
    }
}

/// Errors that can occur during registry operations
#[derive(Debug, thiserror::Error)]
pub enum EffectRegistryError {
    #[error("Registry lock error")]
    LockError,
    #[error("Handler not found: {effect_type}.{operation}")]
    HandlerNotFound { effect_type: String, operation: String },
    #[error("Handler type mismatch")]
    TypeMismatch,
    #[error("Registration failed: {reason}")]
    RegistrationFailed { reason: String },
}

/// Extension trait for EffectRegistry with convenience methods
pub trait EffectRegistryExt {
    /// Register a crypto handler
    fn register_crypto_handler<T: Send + Sync + 'static>(
        &self,
        operation: &str,
        handler: T,
    ) -> Result<(), EffectRegistryError>;

    /// Register a storage handler
    fn register_storage_handler<T: Send + Sync + 'static>(
        &self,
        operation: &str,
        handler: T,
    ) -> Result<(), EffectRegistryError>;

    /// Register a transport handler
    fn register_transport_handler<T: Send + Sync + 'static>(
        &self,
        operation: &str,
        handler: T,
    ) -> Result<(), EffectRegistryError>;

    /// Register a journal handler
    fn register_journal_handler<T: Send + Sync + 'static>(
        &self,
        operation: &str,
        handler: T,
    ) -> Result<(), EffectRegistryError>;
}

impl EffectRegistryExt for EffectRegistry {
    fn register_crypto_handler<T: Send + Sync + 'static>(
        &self,
        operation: &str,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        self.register("crypto".to_string(), operation.to_string(), handler)
    }

    fn register_storage_handler<T: Send + Sync + 'static>(
        &self,
        operation: &str,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        self.register("storage".to_string(), operation.to_string(), handler)
    }

    fn register_transport_handler<T: Send + Sync + 'static>(
        &self,
        operation: &str,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        self.register("transport".to_string(), operation.to_string(), handler)
    }

    fn register_journal_handler<T: Send + Sync + 'static>(
        &self,
        operation: &str,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        self.register("journal".to_string(), operation.to_string(), handler)
    }
}