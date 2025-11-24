//! Effect container for runtime composition
//!
//! Provides container infrastructure for managing effect handler instances
//! and their lifecycle within the authority-centric runtime architecture.

use aura_core::effects::ExecutionMode;
use aura_core::identifiers::AuthorityId;
use aura_core::AuraError;
use std::collections::HashMap;
use std::sync::Arc;

/// Container for effect handler instances
#[allow(dead_code)]
#[derive(Debug)]
pub struct EffectContainer {
    authority_id: AuthorityId,
    execution_mode: ExecutionMode,
    handlers: HashMap<String, Arc<dyn std::any::Any + Send + Sync>>,
}

#[allow(dead_code)]
impl EffectContainer {
    /// Create a new effect container for the given authority
    pub fn new(authority_id: AuthorityId, execution_mode: ExecutionMode) -> Self {
        Self {
            authority_id,
            execution_mode,
            handlers: HashMap::new(),
        }
    }

    /// Register a handler with the container
    pub fn register_handler<T: Send + Sync + 'static>(
        &mut self,
        name: String,
        handler: T,
    ) -> Result<(), AuraError> {
        self.handlers.insert(name, Arc::new(handler));
        Ok(())
    }

    /// Get a handler by name and type
    pub fn get_handler<T: Send + Sync + 'static>(&self, name: &str) -> Option<Arc<T>> {
        self.handlers.get(name)?.clone().downcast::<T>().ok()
    }

    /// Get all handler names
    pub fn handler_names(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Get the execution mode
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Clear all handlers
    pub fn clear(&mut self) {
        self.handlers.clear();
    }

    /// Check if container has a handler
    pub fn has_handler(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }
}

impl Clone for EffectContainer {
    fn clone(&self) -> Self {
        Self {
            authority_id: self.authority_id,
            execution_mode: self.execution_mode,
            handlers: self.handlers.clone(),
        }
    }
}
