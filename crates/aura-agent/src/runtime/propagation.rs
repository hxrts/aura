//! Context propagation utilities
//!
//! Provides context propagation mechanisms for distributed operations
//! and cross-service communication in the authority-centric runtime.

use super::EffectContext;
use aura_core::identifiers::{AuthorityId, ContextId};
use std::collections::HashMap;

/// Context propagation manager
#[derive(Debug)]
#[allow(dead_code)] // Part of future context propagation API
pub struct ContextPropagator {
    active_contexts: HashMap<ContextId, EffectContext>,
}

impl ContextPropagator {
    /// Create a new context propagator
    #[allow(dead_code)] // Part of future context propagation API
    pub fn new() -> Self {
        Self {
            active_contexts: HashMap::new(),
        }
    }

    /// Register a context for propagation
    #[allow(dead_code)] // Part of future context propagation API
    pub fn register_context(&mut self, context: EffectContext) {
        let context_id = context.context_id();
        self.active_contexts.insert(context_id, context);
    }

    /// Get a context by ID
    #[allow(dead_code)] // Part of future context propagation API
    pub fn get_context(&self, context_id: ContextId) -> Option<&EffectContext> {
        self.active_contexts.get(&context_id)
    }

    /// Remove a context
    #[allow(dead_code)] // Part of future context propagation API
    pub fn remove_context(&mut self, context_id: ContextId) -> Option<EffectContext> {
        self.active_contexts.remove(&context_id)
    }

    /// Propagate context to remote operation
    #[allow(dead_code)] // Part of future context propagation API
    pub fn serialize_context(&self, context_id: ContextId) -> Option<ContextSnapshot> {
        self.get_context(context_id).map(|ctx| ContextSnapshot {
            authority_id: ctx.authority_id(),
            context_id: ctx.context_id(),
            metadata: ctx.metadata().clone(),
        })
    }

    /// Restore context from remote operation
    #[allow(dead_code)] // Part of future context propagation API
    pub fn deserialize_context(&self, snapshot: ContextSnapshot) -> EffectContext {
        let mut context = EffectContext::new(
            snapshot.authority_id,
            snapshot.context_id,
            aura_core::effects::ExecutionMode::Production, // Default
        );

        // Restore metadata
        for (key, value) in snapshot.metadata {
            context.set_metadata(key, value);
        }

        context
    }
}

impl Default for ContextPropagator {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable context snapshot for propagation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)] // Part of future context propagation API
pub struct ContextSnapshot {
    pub authority_id: AuthorityId,
    pub context_id: ContextId,
    pub metadata: HashMap<String, String>,
}

/// Context propagation error
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // Part of future context propagation API
pub enum PropagationError {
    #[error("Context not found: {context_id:?}")]
    ContextNotFound { context_id: ContextId },
    #[error("Serialization error: {reason}")]
    SerializationError { reason: String },
    #[error("Deserialization error: {reason}")]
    DeserializationError { reason: String },
}
