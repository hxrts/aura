//! Effect registry for dynamic handler lookup
//!
//! Provides dynamic registry for effect handlers, enabling runtime composition
//! and protocol-specific handler selection in the authority-centric architecture.
//!
//! # Blocking Lock Usage
//!
//! Uses `std::sync::RwLock` (not tokio or parking_lot) because:
//! 1. Lock poisoning detection is required - the code handles `PoisonError` explicitly
//! 2. Operations are brief HashMap lookups/inserts (sub-millisecond)
//! 3. No `.await` points inside lock scope

#![allow(clippy::disallowed_types)]

use aura_core::effects::ExecutionMode;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, RwLock};

use super::executor::EffectHandler;

/// Typed effect categories for registry keys.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum EffectType {
    Crypto,
    Storage,
    Transport,
    Journal,
}

impl EffectType {
    pub const fn as_str(self) -> &'static str {
        match self {
            EffectType::Crypto => "crypto",
            EffectType::Storage => "storage",
            EffectType::Transport => "transport",
            EffectType::Journal => "journal",
        }
    }
}

impl fmt::Display for EffectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Typed operation identifier for registry keys.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct EffectOperation(&'static str);

impl EffectOperation {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for EffectOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl From<&'static str> for EffectOperation {
    fn from(value: &'static str) -> Self {
        Self(value)
    }
}

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

    /// Register a typed effect handler for dynamic dispatch.
    pub fn register_effect_handler<T, H>(
        &self,
        effect_type: EffectType,
        operation: EffectOperation,
        handler: H,
    ) -> Result<(), EffectRegistryError>
    where
        T: Send + Sync + 'static,
        H: EffectHandler<T> + 'static,
    {
        let handler: Arc<dyn EffectHandler<T>> = Arc::new(handler);
        self.register(effect_type, operation, handler)
    }

    /// Retrieve a typed effect handler for dynamic dispatch.
    pub fn get_effect_handler<T: Send + Sync + 'static>(
        &self,
        effect_type: EffectType,
        operation: EffectOperation,
    ) -> Result<Option<Arc<dyn EffectHandler<T>>>, EffectRegistryError> {
        Ok(self
            .get::<Arc<dyn EffectHandler<T>>>(effect_type, operation)?
            .map(|handler| handler.as_ref().clone()))
    }

    /// Register an effect handler
    pub fn register<T: Send + Sync + 'static>(
        &self,
        effect_type: EffectType,
        operation: EffectOperation,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        let key = EffectKey::new(effect_type, operation);

        let mut handlers = self
            .handlers
            .write()
            .map_err(|_| EffectRegistryError::LockError)?;

        handlers.insert(key, Arc::new(handler));
        Ok(())
    }

    /// Get an effect handler
    pub fn get<T: Send + Sync + 'static>(
        &self,
        effect_type: EffectType,
        operation: EffectOperation,
    ) -> Result<Option<Arc<T>>, EffectRegistryError> {
        let key = EffectKey::new(effect_type, operation);

        let handlers = self
            .handlers
            .read()
            .map_err(|_| EffectRegistryError::LockError)?;

        Ok(handlers
            .get(&key)
            .and_then(|handler| handler.clone().downcast::<T>().ok()))
    }

    /// Check if an effect handler is registered
    pub fn has_handler(&self, effect_type: EffectType, operation: EffectOperation) -> bool {
        let key = EffectKey::new(effect_type, operation);

        self.handlers
            .read()
            .map(|handlers| handlers.contains_key(&key))
            .unwrap_or(false)
    }

    /// Get all registered effect types
    pub fn effect_types(&self) -> Vec<EffectType> {
        self.handlers
            .read()
            .map(|handlers| {
                handlers
                    .keys()
                    .map(|key| key.effect_type)
                    .collect::<HashSet<_>>()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get operations for an effect type
    pub fn operations(&self, effect_type: EffectType) -> Vec<EffectOperation> {
        self.handlers
            .read()
            .map(|handlers| {
                handlers
                    .keys()
                    .filter(|key| key.effect_type == effect_type)
                    .map(|key| key.operation)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clear all handlers
    pub fn clear(&self) -> Result<(), EffectRegistryError> {
        let mut handlers = self
            .handlers
            .write()
            .map_err(|_| EffectRegistryError::LockError)?;

        handlers.clear();
        Ok(())
    }

    /// Get the execution mode
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

impl Clone for EffectRegistry {
    fn clone(&self) -> Self {
        Self {
            handlers: self.handlers.clone(),
            execution_mode: self.execution_mode,
        }
    }
}

/// Key for indexing effect handlers
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
struct EffectKey {
    effect_type: EffectType,
    operation: EffectOperation,
}

impl EffectKey {
    fn new(effect_type: EffectType, operation: EffectOperation) -> Self {
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
    HandlerNotFound {
        effect_type: EffectType,
        operation: EffectOperation,
    },
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
        operation: EffectOperation,
        handler: T,
    ) -> Result<(), EffectRegistryError>;

    /// Register a storage handler
    fn register_storage_handler<T: Send + Sync + 'static>(
        &self,
        operation: EffectOperation,
        handler: T,
    ) -> Result<(), EffectRegistryError>;

    /// Register a transport handler
    fn register_transport_handler<T: Send + Sync + 'static>(
        &self,
        operation: EffectOperation,
        handler: T,
    ) -> Result<(), EffectRegistryError>;

    /// Register a journal handler
    fn register_journal_handler<T: Send + Sync + 'static>(
        &self,
        operation: EffectOperation,
        handler: T,
    ) -> Result<(), EffectRegistryError>;
}

impl EffectRegistryExt for EffectRegistry {
    fn register_crypto_handler<T: Send + Sync + 'static>(
        &self,
        operation: EffectOperation,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        self.register(EffectType::Crypto, operation, handler)
    }

    fn register_storage_handler<T: Send + Sync + 'static>(
        &self,
        operation: EffectOperation,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        self.register(EffectType::Storage, operation, handler)
    }

    fn register_transport_handler<T: Send + Sync + 'static>(
        &self,
        operation: EffectOperation,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        self.register(EffectType::Transport, operation, handler)
    }

    fn register_journal_handler<T: Send + Sync + 'static>(
        &self,
        operation: EffectOperation,
        handler: T,
    ) -> Result<(), EffectRegistryError> {
        self.register(EffectType::Journal, operation, handler)
    }
}
